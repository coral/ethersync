//! Local deadlines evaluated against one immutable reader snapshot.
use crate::{Position, SyncState, timeline::View};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    /// The next strictly future integer-position crossing in the playback direction.
    Frame,
    /// A scheduled control takes effect; recompute the next deadline after waking.
    ScheduledChange,
}
#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    pub local_deadline_ns: u64,
    /// Exact integer target for a frame crossing; evaluated position for a control.
    pub position: Position,
    pub discontinuity: u64,
    pub kind: BoundaryKind,
    /// Clock contribution in local time, excluding source and presentation latency.
    pub uncertainty_ns: f64,
}
// First integer nanosecond satisfying a monotonic predicate, without rounding early.
fn first(mut lo: u64, mut hi: u64, predicate: impl Fn(u64) -> bool) -> Option<u64> {
    if lo > hi || !predicate(hi) {
        return None;
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if predicate(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Some(lo)
}
impl View {
    /// Allocation-free prediction from this snapshot. New mappings or controls invalidate it.
    /// Returns a scheduled-change wakeup before any frame it supersedes. A paused timeline
    /// without pending controls, an uninitialized clock, or an exhausted time range returns None.
    pub fn next_boundary(self, local: u64) -> Option<Boundary> {
        let r = self.evaluate(local);
        let start = local.checked_add(1)?;
        let end = i64::MAX as u64;
        if start > end || r.status.synchronization == SyncState::Uninitialized {
            return None;
        }
        let leader_now = self.mapping.leader_time(local);
        let scheduled = self.timeline.scheduled[..self.timeline.scheduled_len]
            .iter()
            .find(|s| s.anchor.time_ns > leader_now)
            .and_then(|s| {
                first(start, end, |t| {
                    self.mapping.leader_time(t) >= s.anchor.time_ns
                })
            });
        let frame_end = scheduled.map_or(end, |t| t - 1);
        let fixed = r.position.fixed();
        let unit = 1i128 << 32;
        let forward = r.rate.numerator() > 0;
        let target = if forward {
            (fixed.div_euclid(unit) + 1) * unit
        } else {
            (fixed.div_euclid(unit) - i128::from(fixed.rem_euclid(unit) == 0)) * unit
        };
        let frame = if r.rate.numerator() == 0 {
            None
        } else {
            first(start, frame_end, |t| {
                let p = self.evaluate(t).position.fixed();
                if forward { p >= target } else { p <= target }
            })
        };
        let (deadline, kind, position) = if let Some(t) = frame {
            (t, BoundaryKind::Frame, Position::from_fixed(target))
        } else {
            let t = scheduled?;
            (t, BoundaryKind::ScheduledChange, self.evaluate(t).position)
        };
        Some(Boundary {
            local_deadline_ns: deadline,
            position,
            discontinuity: self.evaluate(deadline).discontinuity,
            kind,
            // Slew can reduce phase velocity by at most 10%; controls do not slew.
            uncertainty_ns: (if self.local_clock {
                0.
            } else {
                self.mapping.uncertainty_at(deadline)
            }) / (1. + self.mapping.drift
                - if kind == BoundaryKind::Frame && r.status.correction_frames != 0. {
                    0.1
                } else {
                    0.
                }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FrameFormat, Rate,
        clock::ClockMapping,
        timeline::{Anchor, Scheduled},
    };
    fn view(rate: Rate) -> View {
        let mut v = View {
            sync: SyncState::Synchronized,
            mapping: ClockMapping {
                last_sample_ns: 1,
                uncertainty_ns: 100_000.,
                ..Default::default()
            },
            ..Default::default()
        };
        v.timeline.anchor.rate = rate;
        v
    }
    #[test]
    fn exact_crossings_fractional_reverse_slew_and_drift() {
        for (num, den, drop) in [
            (30, 1, false),
            (24000, 1001, false),
            (30000, 1001, true),
            (60000, 1001, true),
        ] {
            for (n, d) in [(1, 1), (-1, 1), (2, 1), (1, 10000), (-1, 10000)] {
                for correction in [-0.2, 0., 0.2] {
                    let mut v = view(Rate::new(n, d).unwrap());
                    v.timeline.format = FrameFormat::new(num, den, drop).unwrap();
                    v.timeline.anchor.position = Position::from_frames(-5);
                    v.mapping.offset_ns = 1_000_000.;
                    v.mapping.drift = 0.0002;
                    v.correction_frames = correction;
                    let b = v.next_boundary(1_000_000).unwrap();
                    assert_eq!(b.kind, BoundaryKind::Frame);
                    assert!(b.local_deadline_ns > 1_000_000);
                    assert_eq!(b.position.subframe, 0);
                    let before = v.evaluate(b.local_deadline_ns - 1).position;
                    let at = v.evaluate(b.local_deadline_ns).position;
                    if n > 0 {
                        assert!(before < b.position && at >= b.position);
                    } else {
                        assert!(before > b.position && at <= b.position);
                    }
                }
            }
        }
    }
    #[test]
    fn exact_boundary_is_strictly_future_and_midnight_is_unwrapped() {
        let mut v = view(Rate::NORMAL);
        v.timeline.anchor.position = Position::from_frames(30 * 86400 - 1);
        let b = v.next_boundary(0).unwrap();
        assert_eq!(b.position.frames, 30 * 86400);
        assert_eq!(
            v.timeline.format.label(b.position).to_string(),
            "00:00:00:00"
        );
        assert_eq!(b.local_deadline_ns, 33_333_334);
        v.timeline.anchor.rate = Rate::new(-1, 1).unwrap();
        assert_eq!(v.next_boundary(0).unwrap().position.frames, 30 * 86400 - 2);
    }
    #[test]
    fn paused_and_schedule_preemption() {
        let mut v = view(Rate::PAUSED);
        assert!(v.next_boundary(0).is_none());
        v.timeline.scheduled_len = 1;
        v.timeline.scheduled[0] = Scheduled {
            discontinuity: 1,
            anchor: Anchor {
                time_ns: 10_000_000,
                position: Position::from_frames(40),
                rate: Rate::NORMAL,
            },
        };
        let b = v.next_boundary(0).unwrap();
        assert_eq!(b.kind, BoundaryKind::ScheduledChange);
        assert_eq!(b.local_deadline_ns, 10_000_000);
        assert_eq!(b.discontinuity, 1);
        assert_eq!(b.position.frames, 40);
        assert_eq!(
            v.next_boundary(b.local_deadline_ns).unwrap().kind,
            BoundaryKind::Frame
        );
        v.timeline.anchor.rate = Rate::NORMAL;
        assert_eq!(
            v.next_boundary(0).unwrap().kind,
            BoundaryKind::ScheduledChange
        );
        v.timeline.scheduled[0].anchor.rate = Rate::PAUSED;
        assert!(v.next_boundary(10_000_000).is_none());
        assert!(View::default().next_boundary(0).is_none());
        assert!(v.next_boundary(i64::MAX as u64).is_none());
    }
}
