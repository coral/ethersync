//! Timestamped trajectories, scheduled discontinuities, and follower correction policy.
use crate::clock::ClockMapping;
use crate::{Error, wire};
use crate::{FrameFormat, Position, Rate};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Shutdown,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncState {
    #[default]
    Uninitialized,
    Acquiring,
    Synchronized,
    Holdover,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceKind {
    #[default]
    Generated,
    Tracked,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceHealth {
    #[default]
    Healthy,
    Degraded,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Correction {
    Discontinuity(u64),
    Slew { frames: f64 },
    HardResync { frames: f64 },
    NewSession,
}
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub time_ns: u64,
    pub position: Position,
    pub rate: Rate,
}
impl Default for Anchor {
    fn default() -> Self {
        Self {
            time_ns: 0,
            position: Position::ZERO,
            rate: Rate::PAUSED,
        }
    }
}
impl Anchor {
    pub fn at(self, time: u64, format: FrameFormat) -> Position {
        self.position.advance(
            (time as i128 - self.time_ns as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            format,
            self.rate,
        )
    }
    pub fn wire(self) -> wire::Anchor {
        wire::Anchor {
            time_ns: self.time_ns,
            position: Some(wire::Position {
                frames: self.position.frames,
                subframe: self.position.subframe,
            }),
            rate: Some(wire::Rate {
                numerator: self.rate.numerator(),
                denominator: self.rate.denominator(),
            }),
        }
    }
    fn from_wire(a: &wire::Anchor) -> Result<Self, Error> {
        let p = a.position.as_ref().ok_or(Error::Invalid("position"))?;
        let r = a.rate.as_ref().ok_or(Error::Invalid("rate"))?;
        Ok(Self {
            time_ns: a.time_ns,
            position: Position {
                frames: p.frames,
                subframe: p.subframe,
            },
            rate: Rate::new(r.numerator, r.denominator)?,
        })
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Scheduled {
    pub discontinuity: u64,
    pub anchor: Anchor,
}
#[derive(Clone, Copy, Debug)]
pub struct Timeline {
    pub session: [u8; 16],
    pub revision: u64,
    pub discontinuity: u64,
    pub format: FrameFormat,
    pub anchor: Anchor,
    pub source_kind: SourceKind,
    pub source_health: SourceHealth,
    pub scheduled: [Scheduled; 4],
    pub scheduled_len: usize,
}
impl Default for Timeline {
    fn default() -> Self {
        Self {
            session: [0; 16],
            revision: 0,
            discontinuity: 0,
            format: FrameFormat::default(),
            anchor: Anchor::default(),
            source_kind: SourceKind::Generated,
            source_health: SourceHealth::Healthy,
            scheduled: [Scheduled::default(); 4],
            scheduled_len: 0,
        }
    }
}
impl Timeline {
    pub fn at(&self, time: u64) -> (Position, Rate, u64) {
        let mut a = self.anchor;
        let mut d = self.discontinuity;
        for s in &self.scheduled[..self.scheduled_len] {
            if s.anchor.time_ns <= time {
                a = s.anchor;
                d = s.discontinuity;
            }
        }
        (a.at(time, self.format), a.rate, d)
    }
    fn latch(&mut self, discontinuity: u64) {
        while self.scheduled_len > 0 && self.scheduled[0].discontinuity <= discontinuity {
            let next = self.scheduled[0];
            self.anchor = next.anchor;
            self.discontinuity = next.discontinuity;
            self.scheduled.rotate_left(1);
            self.scheduled_len -= 1;
        }
    }
    pub fn wire(&self) -> wire::Snapshot {
        wire::Snapshot {
            version: 1,
            session: self.session.to_vec(),
            revision: self.revision,
            discontinuity: self.discontinuity,
            source_kind: match self.source_kind {
                SourceKind::Generated => 1,
                SourceKind::Tracked => 2,
            },
            source_health: match self.source_health {
                SourceHealth::Healthy => 1,
                SourceHealth::Degraded => 2,
            },
            format: Some(wire::FrameFormat {
                numerator: self.format.numerator(),
                denominator: self.format.denominator(),
                drop_frame: self.format.drop_frame(),
            }),
            anchor: Some(self.anchor.wire()),
            scheduled: self.scheduled[..self.scheduled_len]
                .iter()
                .map(|s| wire::ScheduledChange {
                    discontinuity: s.discontinuity,
                    anchor: Some(s.anchor.wire()),
                })
                .collect(),
        }
    }
    pub fn from_wire(s: &wire::Snapshot) -> Result<Self, Error> {
        crate::validate_snapshot(s)?;
        let f = s.format.as_ref().unwrap();
        let mut t = Self {
            session: s.session.as_slice().try_into().unwrap(),
            revision: s.revision,
            discontinuity: s.discontinuity,
            format: FrameFormat::new(f.numerator, f.denominator, f.drop_frame)?,
            anchor: Anchor::from_wire(s.anchor.as_ref().unwrap())?,
            source_kind: if s.source_kind == 1 {
                SourceKind::Generated
            } else {
                SourceKind::Tracked
            },
            source_health: if s.source_health == 1 {
                SourceHealth::Healthy
            } else {
                SourceHealth::Degraded
            },
            scheduled_len: s.scheduled.len(),
            ..Self::default()
        };
        for (dst, src) in t.scheduled.iter_mut().zip(&s.scheduled) {
            *dst = Scheduled {
                discontinuity: src.discontinuity,
                anchor: Anchor::from_wire(src.anchor.as_ref().unwrap())?,
            };
        }
        Ok(t)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub connection: ConnectionState,
    pub synchronization: SyncState,
    pub source_kind: SourceKind,
    pub source_health: SourceHealth,
    /// Clock-mapping uncertainty; excludes intentional timeline slew.
    pub uncertainty_ns: f64,
    /// Remaining signed position adjustment relative to the current clock mapping.
    pub correction_frames: f64,
    pub offset_evidence: Option<crate::clock::OffsetEvidence>,
    pub sample_age_ns: u64,
    pub rtt_ns: u64,
    pub offset_ns: f64,
    pub drift_ppm: f64,
    pub lost_packets: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Reading {
    pub position: Position,
    pub format: FrameFormat,
    pub rate: Rate,
    pub discontinuity: u64,
    pub status: Status,
}
impl Reading {
    pub fn label(self) -> crate::Label {
        self.format.label(self.position)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub timeline: Timeline,
    pub fallback_position: Position,
    pub fallback_format: FrameFormat,
    pub mapping: ClockMapping,
    pub connection: ConnectionState,
    pub sync: SyncState,
    pub rtt_ns: u64,
    pub lost_packets: u64,
    pub correction_frames: f64,
    pub correction_at: u64,
    pub slew_frames_per_second: f64,
    pub correction_discontinuity: u64,
}
impl Default for View {
    fn default() -> Self {
        Self {
            timeline: Timeline::default(),
            fallback_position: Position::ZERO,
            fallback_format: FrameFormat::default(),
            mapping: ClockMapping::default(),
            connection: ConnectionState::Disconnected,
            sync: SyncState::Uninitialized,
            rtt_ns: 0,
            lost_packets: 0,
            correction_frames: 0.,
            correction_at: 0,
            slew_frames_per_second: 0.03,
            correction_discontinuity: 0,
        }
    }
}
impl View {
    /// Predict output for a positive local presentation delay. This evaluates the whole
    /// trajectory at the presentation instant, including drift, slew and known controls.
    /// Pure prediction: does not advance follower state or add network delay a second time.
    pub fn evaluate_for_presentation(
        self,
        local: u64,
        compensation_delay_ns: u64,
    ) -> Result<Reading, Error> {
        let at = presentation_time(local, compensation_delay_ns)?;
        Ok(self.evaluate(at))
    }
    pub fn evaluate(self, local: u64) -> Reading {
        let time = if self.sync == SyncState::Uninitialized {
            self.timeline.anchor.time_ns
        } else {
            self.mapping.leader_time(local)
        };
        let (mut position, rate, discontinuity) = self.timeline.at(time);
        // Phase correction must not overpower shuttle motion. Paused output stays
        // fixed until an explicit discontinuity (or a confirmed hard resync).
        let slew = self
            .slew_frames_per_second
            .min(self.timeline.format.fps() * rate.as_f64().abs() * 0.1);
        let remaining = if discontinuity == self.correction_discontinuity {
            (self.correction_frames.abs()
                - local.saturating_sub(self.correction_at) as f64 / 1e9 * slew)
                .max(0.)
                * self.correction_frames.signum()
        } else {
            0.
        };
        position = Position::from_fixed(position.fixed() + (remaining * 4294967296.) as i128);
        let initialized = self.mapping.last_sample_ns != 0;
        Reading {
            position: if initialized {
                position
            } else {
                self.fallback_position
            },
            format: if initialized {
                self.timeline.format
            } else {
                self.fallback_format
            },
            rate: if initialized { rate } else { Rate::PAUSED },
            discontinuity,
            status: Status {
                connection: self.connection,
                synchronization: if initialized {
                    self.sync
                } else {
                    SyncState::Uninitialized
                },
                source_kind: self.timeline.source_kind,
                source_health: self.timeline.source_health,
                uncertainty_ns: self.mapping.uncertainty_at(local),
                correction_frames: if initialized { remaining } else { 0. },
                offset_evidence: self.mapping.evidence.map(|e| e.at(local)),
                sample_age_ns: local.saturating_sub(self.mapping.last_sample_ns),
                rtt_ns: self.rtt_ns,
                offset_ns: self.mapping.offset_at(local),
                drift_ppm: self.mapping.drift * 1e6,
                lost_packets: self.lost_packets,
            },
        }
    }
}
/// Checked local timestamp for predicted presentation; delays must be nonnegative.
pub fn presentation_time(local: u64, compensation_delay_ns: u64) -> Result<u64, Error> {
    local
        .checked_add(compensation_delay_ns)
        .filter(|at| *at <= i64::MAX as u64)
        .ok_or(Error::Invalid(
            "presentation timestamp exceeds supported clock range",
        ))
}
#[derive(Clone, Copy, Debug)]
pub struct CorrectionPolicy {
    pub slew_frames_per_second: f64,
    pub hard_threshold_frames: f64,
    pub confirmations: u8,
}
impl Default for CorrectionPolicy {
    fn default() -> Self {
        Self {
            slew_frames_per_second: 0.03,
            hard_threshold_frames: 1.,
            confirmations: 3,
        }
    }
}
/// Transport-independent follower. Supply validated timelines and matched probe exchanges.
/// Call `connected` for each new connection, `tick` for staleness/scheduled events,
/// and `disconnected` on loss. Copy `view` to evaluate without allocation or locks.
pub struct FollowerCore {
    pub view: View,
    pub estimator: crate::clock::ClockEstimator,
    pub policy: CorrectionPolicy,
    large: u8,
    large_sign: i8,
    ever_locked: bool,
    expected_session: Option<[u8; 16]>,
    reported_discontinuity: Option<([u8; 16], u64)>,
}
impl FollowerCore {
    pub fn new(fallback: Timeline, policy: CorrectionPolicy) -> Self {
        Self {
            view: View {
                timeline: fallback,
                fallback_position: fallback.anchor.position,
                fallback_format: fallback.format,
                slew_frames_per_second: policy.slew_frames_per_second,
                ..View::default()
            },
            estimator: Default::default(),
            policy,
            large: 0,
            large_sign: 0,
            ever_locked: false,
            expected_session: None,
            reported_discontinuity: None,
        }
    }
    /// Called once for each newly established transport; permits a new leader session.
    pub fn connected(&mut self) {
        self.large = 0;
        self.large_sign = 0;
        self.expected_session = None;
        self.view.connection = ConnectionState::Connected;
    }
    pub fn state(&mut self, mut t: Timeline, now: u64) -> Option<Correction> {
        if self.expected_session.is_some_and(|s| s != t.session) {
            return None;
        }
        self.expected_session = Some(t.session);
        let new = self.view.timeline.session != t.session;
        if !new && t.revision <= self.view.timeline.revision {
            return None;
        }
        let before = self.view.evaluate(now);
        if !new {
            t.latch(self.view.timeline.discontinuity.max(before.discontinuity));
            // A newer revision may retain schedules we already applied, but cannot undo one.
            if t.discontinuity < before.discontinuity
                || (t.format != self.view.timeline.format
                    && t.discontinuity == before.discontinuity)
            {
                return None;
            }
        }
        if new {
            self.estimator = Default::default();
            self.view.mapping = ClockMapping::default();
            self.view.sync = SyncState::Acquiring;
            self.large = 0;
            self.large_sign = 0;
            self.ever_locked = false;
        }
        let changed = new || t.at(self.view.mapping.leader_time(now)).2 != before.discontinuity;
        self.view.timeline = t;
        if changed {
            self.large = 0;
            self.large_sign = 0;
            self.view.correction_frames = 0.;
            return Some(if new {
                Correction::NewSession
            } else {
                let disc = t.at(self.view.mapping.leader_time(now)).2;
                self.reported_discontinuity = Some((t.session, disc));
                Correction::Discontinuity(disc)
            });
        }
        self.correct(before.position, now, false)
    }
    pub fn measurement(&mut self, e: crate::clock::Exchange) -> Option<Correction> {
        self.measurement_timed(e, None, e.t4)
    }
    /// Diagnostic timestamps never replace the original four timing observations.
    pub fn measurement_timed(
        &mut self,
        e: crate::clock::Exchange,
        publication_ns: Option<u64>,
        processed_ns: u64,
    ) -> Option<Correction> {
        self.expected_session?;
        let before = self.view.evaluate(e.t4);
        if !self
            .estimator
            .observe_timed(e, publication_ns, processed_ns)
        {
            return None;
        }
        let acquired = self.ever_locked;
        self.view.mapping = self.estimator.mapping();
        self.ever_locked |= self.view.mapping.converged;
        self.view.sync = if self.view.mapping.converged {
            SyncState::Synchronized
        } else {
            SyncState::Acquiring
        };
        // Acquisition estimates can move substantially as startup queueing clears.
        // Do not preserve those provisional errors with the steady-state slew limiter.
        // Include the first converged estimate in acquisition so lock starts aligned.
        if !acquired {
            self.view.correction_frames = 0.;
            let desired = self.view.timeline.at(self.view.mapping.leader_time(e.t4)).0;
            return Some(Correction::HardResync {
                frames: (desired.fixed() - before.position.fixed()) as f64 / 4294967296.,
            });
        }
        if self.view.timeline.at(self.view.mapping.leader_time(e.t4)).2 != before.discontinuity {
            self.large = 0;
            self.large_sign = 0;
            self.view.correction_frames = 0.;
            let disc = self.view.timeline.at(self.view.mapping.leader_time(e.t4)).2;
            self.reported_discontinuity = Some((self.view.timeline.session, disc));
            return Some(Correction::Discontinuity(disc));
        }
        self.correct(before.position, e.t4, true)
    }
    fn correct(&mut self, before: Position, now: u64, measurement: bool) -> Option<Correction> {
        let (desired, _, disc) = self.view.timeline.at(self.view.mapping.leader_time(now));
        let error = (before.fixed() - desired.fixed()) as f64 / 4294967296.;
        if error.abs() > self.policy.hard_threshold_frames {
            let sign = if error > 0. { 1 } else { -1 };
            if sign != self.large_sign {
                self.large = 0;
            }
            self.large_sign = sign;
            if measurement {
                self.large = self.large.saturating_add(1);
            }
            if self.large >= self.policy.confirmations {
                self.large = 0;
                self.view.correction_frames = 0.;
                return Some(Correction::HardResync { frames: -error });
            }
        } else {
            self.large = 0;
        }
        self.view.correction_frames = error;
        self.view.correction_at = now;
        self.view.correction_discontinuity = disc;
        Some(Correction::Slew { frames: -error })
    }
    pub fn tick(&mut self, now: u64, stale_after: u64) -> Option<Correction> {
        if self.view.mapping.last_sample_ns == 0 {
            return None;
        }
        if now.saturating_sub(self.view.mapping.last_sample_ns) > stale_after {
            self.view.sync = SyncState::Holdover;
        }
        let disc = self.view.timeline.at(self.view.mapping.leader_time(now)).2;
        self.view.timeline.latch(disc);
        let key = (self.view.timeline.session, disc);
        let previous = self.reported_discontinuity.replace(key);
        if previous.is_some_and(|p| p.0 == key.0 && p.1 != key.1) {
            self.large = 0;
            self.large_sign = 0;
            Some(Correction::Discontinuity(key.1))
        } else {
            None
        }
    }
    pub fn disconnected(&mut self) {
        self.view.connection = ConnectionState::Disconnected;
        if self.view.mapping.last_sample_ns != 0 {
            self.view.sync = SyncState::Holdover;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Timeline {
        Timeline {
            session: [1; 16],
            revision: 1,
            anchor: Anchor {
                time_ns: 1_000_000_000,
                position: Position::from_frames(30),
                rate: Rate::NORMAL,
            },
            ..Default::default()
        }
    }
    fn core() -> FollowerCore {
        let mut c = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
        c.connected();
        c.state(state(), 1_000_000_000);
        for i in 0..40 {
            let t = 1_000_000_000 + i * 50_000_000;
            c.measurement(crate::clock::Exchange {
                t1: t,
                t2: t + 1_000_000,
                t3: t + 1_010_000,
                t4: t + 2_010_000,
            });
        }
        c
    }
    #[test]
    fn stale_session_revision_and_restart() {
        let mut c = core();
        let mut t = state();
        t.anchor.position = Position::from_frames(9000);
        assert!(c.state(t, 3_000_000_000).is_none());
        assert_ne!(c.view.timeline.anchor.position, t.anchor.position);
        t.session = [2; 16];
        assert!(c.state(t, 3_000_000_000).is_none());
        c.connected();
        assert_eq!(c.state(t, 3_000_000_000), Some(Correction::NewSession));
        assert!(!c.view.mapping.converged);
        assert!(c.state(state(), 3_000_000_000).is_none());
    }
    #[test]
    fn late_discontinuity_is_immediate_and_once() {
        let mut c = core();
        let mut t = state();
        t.revision = 2;
        t.discontinuity = 1;
        t.anchor = Anchor {
            time_ns: 2_000_000_000,
            position: Position::from_frames(900),
            rate: Rate::new(-2, 1).unwrap(),
        };
        assert_eq!(
            c.state(t, 3_000_000_000),
            Some(Correction::Discontinuity(1))
        );
        assert!((c.view.evaluate(3_000_000_000).position.as_frames() - 840.).abs() < 0.001);
        assert_eq!(c.state(t, 3_000_000_000), None);
    }
    #[test]
    fn scheduled_applies_in_reader_during_holdover() {
        let mut c = core();
        let mut t = state();
        t.revision = 2;
        t.scheduled_len = 1;
        t.scheduled[0] = Scheduled {
            discontinuity: 1,
            anchor: Anchor {
                time_ns: 4_000_000_000,
                position: Position::from_frames(900),
                rate: Rate::PAUSED,
            },
        };
        c.state(t, 3_000_000_000);
        c.disconnected();
        assert_eq!(c.view.evaluate(3_500_000_000).rate, Rate::NORMAL);
        let r = c.view.evaluate(5_000_000_000);
        assert_eq!(r.position, Position::from_frames(900));
        assert_eq!(r.discontinuity, 1);
        assert_eq!(r.status.synchronization, SyncState::Holdover);
    }
    #[test]
    fn bounded_slew_and_three_measurement_resync() {
        let mut c = core();
        let mut t = c.view.timeline;
        t.revision += 1;
        t.anchor.position = Position::from_fixed(t.anchor.position.fixed() + (1 << 30));
        let before = c.view.evaluate(3_000_000_000).position;
        c.state(t, 3_000_000_000);
        assert_eq!(c.view.evaluate(3_000_000_000).position, before);
        let r = c.view.evaluate(4_000_000_000);
        assert!((r.position.as_frames() - before.as_frames() - 30.03).abs() < 0.001);
        t.revision += 1;
        t.anchor.position = Position::from_frames(900);
        c.state(t, 4_000_000_000);
        for i in 0..3 {
            let t = 4_000_000_000 + i * 50_000_000;
            let e = c.measurement(crate::clock::Exchange {
                t1: t,
                t2: t + 1_000_000,
                t3: t + 1_010_000,
                t4: t + 2_010_000,
            });
            assert_eq!(matches!(e, Some(Correction::HardResync { .. })), i == 2);
        }
    }
    #[test]
    fn indefinite_reverse_and_paused_holdover() {
        let mut c = core();
        c.view.timeline.anchor.rate = Rate::new(-1, 2).unwrap();
        c.disconnected();
        let a = c.view.evaluate(4_000_000_000);
        let b = c.view.evaluate(86_404_000_000_000);
        assert!((b.position.as_frames() - a.position.as_frames() + 1_296_000.).abs() < 0.001);
        c.view.timeline.anchor.rate = Rate::PAUSED;
        assert_eq!(
            c.view.evaluate(4_000_000_000).position,
            c.view.evaluate(86_404_000_000_000).position
        );
    }
    #[test]
    fn schedule_is_not_reapplied_after_clock_correction() {
        let mut c = core();
        let mut t = state();
        t.revision = 2;
        t.scheduled_len = 1;
        t.scheduled[0] = Scheduled {
            discontinuity: 1,
            anchor: Anchor {
                time_ns: 4_000_000_000,
                position: Position::from_frames(900),
                rate: Rate::PAUSED,
            },
        };
        c.state(t, 3_000_000_000);
        c.tick(3_000_000_000, u64::MAX);
        assert_eq!(
            c.tick(4_010_000_000, u64::MAX),
            Some(Correction::Discontinuity(1))
        );
        c.view.mapping.offset_ns -= 100_000_000.;
        assert_eq!(
            c.view.evaluate(4_020_000_000).position,
            Position::from_frames(900)
        );
        t.revision = 3;
        c.state(t, 4_020_000_000);
        assert_eq!(c.view.evaluate(4_020_000_000).discontinuity, 1);
        assert_eq!(c.tick(4_030_000_000, u64::MAX), None);
    }
}

#[cfg(test)]
mod accuracy_investigation {
    use super::*;
    use crate::clock::Exchange;

    #[test]
    fn initial_asymmetric_probe_does_not_leave_a_synchronized_timeline_ahead() {
        let mut core = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
        let leader = Timeline {
            session: [1; 16],
            revision: 1,
            anchor: Anchor {
                time_ns: 0,
                position: Position::ZERO,
                rate: Rate::NORMAL,
            },
            ..Default::default()
        };
        core.connected();
        core.state(leader, 1);
        // Clocks actually have identical epochs. Only the first request is delayed 40 ms.
        // That creates a +20 ms initial estimate; subsequent exchanges are symmetric.
        for i in 0..40u64 {
            let t = 1_000_000_000 + i * 50_000_000;
            let up = if i == 0 { 41_000_000 } else { 1_000_000 };
            let e = Exchange {
                t1: t,
                t2: t + up,
                t3: t + up,
                t4: t + up + 1_000_000,
            };
            core.measurement(e);
        }
        let now = 3_000_000_000;
        let reading = core.view.evaluate(now);
        let clock_error_ms = (core.view.mapping.leader_time(now) as f64 - now as f64) / 1e6;
        let timeline_error_ms =
            (reading.position.as_frames() - leader.at(now).0.as_frames()) / 30. * 1000.;
        eprintln!(
            "startup asymmetry: clock error={clock_error_ms:.3}ms timeline error={timeline_error_ms:.3}ms reported uncertainty={:.3}ms status={:?}",
            reading.status.uncertainty_ns / 1e6,
            reading.status.synchronization
        );
        assert!(clock_error_ms.abs() < 0.001);
        assert!(timeline_error_ms.abs() < 0.001);
    }
}

#[cfg(test)]
mod stability_tests {
    use super::*;
    #[test]
    fn a_higher_revision_cannot_undo_a_discontinuity() {
        let mut c = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
        c.connected();
        let t = Timeline {
            session: [1; 16],
            revision: 2,
            discontinuity: 4,
            ..Default::default()
        };
        c.state(t, 1);
        let rollback = Timeline {
            revision: 3,
            discontinuity: 3,
            ..t
        };
        assert!(c.state(rollback, 2).is_none());
        assert_eq!(c.view.timeline.discontinuity, 4);
    }
    #[test]
    fn alternating_errors_do_not_confirm_a_hard_resync() {
        let mut c = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
        for i in 0..8 {
            let before = Position::from_frames(if i % 2 == 0 { 2 } else { -2 });
            assert!(
                !matches!(
                    c.correct(before, i, true),
                    Some(Correction::HardResync { .. })
                ),
                "alternating noise triggered a hard resync"
            );
        }
    }
}

#[cfg(test)]
mod recovery_confirmation_tests {
    use super::*;
    #[test]
    fn reacquisition_still_requires_three_consistent_large_measurements() {
        let mut c = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
        c.connected();
        c.state(
            Timeline {
                session: [1; 16],
                revision: 1,
                anchor: Anchor {
                    rate: Rate::NORMAL,
                    ..Default::default()
                },
                ..Default::default()
            },
            1,
        );
        let e = |t, offset| crate::clock::Exchange {
            t1: t,
            t2: t + offset + 1_000_000,
            t3: t + offset + 1_000_000,
            t4: t + 2_000_000,
        };
        for i in 0..80 {
            c.measurement(e(1_000_000_000 + i * 250_000_000, 100_000_000));
        }
        let mut accepted = 0;
        for i in 0..12 {
            if let Some(correction) =
                c.measurement(e(21_000_000_000 + i * 250_000_000, 200_000_000))
            {
                accepted += 1;
                assert_eq!(
                    matches!(correction, Correction::HardResync { .. }),
                    accepted == 3
                );
            }
        }
        assert!(accepted >= 3);
        assert!(
            c.view
                .evaluate(25_000_000_000)
                .status
                .correction_frames
                .abs()
                < 0.0001
        );
    }
}

#[cfg(test)]
mod format_transition_tests {
    use super::*;
    #[test]
    fn format_changes_require_a_discontinuity() {
        let mut c = FollowerCore::new(Timeline::default(), CorrectionPolicy::default());
        c.connected();
        let t = Timeline {
            session: [1; 16],
            revision: 1,
            ..Default::default()
        };
        c.state(t, 1);
        let mut change = Timeline {
            revision: 2,
            format: FrameFormat::new(25, 1, false).unwrap(),
            ..t
        };
        assert!(c.state(change, 2).is_none());
        assert_eq!(c.view.timeline.format, t.format);
        change.discontinuity = 1;
        assert!(matches!(
            c.state(change, 3),
            Some(Correction::Discontinuity(1))
        ));
        assert_eq!(c.view.timeline.format, change.format);
    }
}

#[cfg(test)]
mod slow_transport_tests {
    use super::*;
    #[test]
    fn slew_does_not_reverse_slow_playback_or_move_a_paused_timeline() {
        for numerator in [-1, 0, 1] {
            let view = View {
                timeline: Timeline {
                    anchor: Anchor {
                        rate: Rate::new(numerator, 10000).unwrap(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                mapping: ClockMapping {
                    last_sample_ns: 1,
                    converged: true,
                    ..Default::default()
                },
                sync: SyncState::Synchronized,
                correction_frames: if numerator < 0 { -0.01 } else { 0.01 },
                correction_at: 1_000_000_000,
                ..Default::default()
            };
            let a = view.evaluate(1_000_000_000).position;
            let b = view.evaluate(1_100_000_000).position;
            if numerator == 0 {
                assert_eq!(a, b);
            } else {
                assert!(
                    (b.fixed() - a.fixed()) * numerator as i128 > 0,
                    "slew reversed slow transport"
                );
            }
        }
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    #[test]
    fn presentation_applies_delay_in_local_clock_before_mapping_and_slew() {
        for n in [-2, 0, 1, 2] {
            let mut v = View {
                sync: SyncState::Synchronized,
                mapping: ClockMapping {
                    reference_ns: 1_000_000_000,
                    offset_ns: 5_000_000_000.,
                    drift: 0.0002,
                    last_sample_ns: 1_000_000_000,
                    ..Default::default()
                },
                correction_frames: 0.5,
                correction_at: 1_000_000_000,
                ..Default::default()
            };
            v.timeline.anchor.rate = Rate::new(n, 1).unwrap();
            v.timeline.format = FrameFormat::new(30000, 1001, true).unwrap();
            let expected = v.evaluate(1_050_000_000);
            let actual = v
                .evaluate_for_presentation(1_000_000_000, 50_000_000)
                .unwrap();
            assert_eq!(actual.position, expected.position);
            assert_eq!(actual.rate, expected.rate);
            assert_eq!(
                actual.status.correction_frames,
                expected.status.correction_frames
            );
            assert_eq!(actual.status.sample_age_ns, 50_000_000);
            assert_eq!(
                v.evaluate_for_presentation(1_000_000_000, 0)
                    .unwrap()
                    .position,
                v.evaluate(1_000_000_000).position
            );
        }
    }
    #[test]
    fn presentation_overflow_is_rejected_and_fallback_remains_paused() {
        let v = View::default();
        assert!(v.evaluate_for_presentation(i64::MAX as u64, 1).is_err());
        assert!(v.evaluate_for_presentation(u64::MAX, 1).is_err());
        let r = v.evaluate_for_presentation(1, 1_000_000_000).unwrap();
        assert_eq!(r.position, Position::ZERO);
        assert_eq!(r.rate, Rate::PAUSED);
        assert_eq!(r.status.synchronization, SyncState::Uninitialized);
    }
}
