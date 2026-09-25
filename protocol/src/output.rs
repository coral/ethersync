//! Output-time conversion and sample evaluation. No device, allocator or runtime.
use crate::{Error, Reading, TimecodeSnapshot, timeline::presentation_time};

#[derive(Clone, Copy, Debug)]
pub struct OutputTime {
    pub local_ns: u64,
    pub uncertainty_ns: f64,
}

/// A bracketed epoch conversion between two readings of the SAME-rate host clock.
/// This is not an estimator for an independent audio-device oscillator. Refresh
/// after suspend or a clock-domain change; supply device host timestamps for audio.
#[derive(Clone, Copy, Debug)]
pub struct ClockBridge {
    offset_ns: i128,
    uncertainty_ns: f64,
}
impl ClockBridge {
    /// external_ns was sampled between local_before_ns and local_after_ns.
    pub fn new(local_before_ns: u64, external_ns: u64, local_after_ns: u64) -> Result<Self, Error> {
        if local_after_ns < local_before_ns || local_after_ns > i64::MAX as u64 {
            return Err(Error::Invalid("clock bridge bracket"));
        }
        let span = local_after_ns - local_before_ns;
        Ok(Self {
            offset_ns: i128::from(local_before_ns + span / 2) - i128::from(external_ns),
            uncertainty_ns: span as f64 / 2. + 1.,
        })
    }
    pub fn convert(self, external_ns: u64) -> Result<OutputTime, Error> {
        let local = i128::from(external_ns) + self.offset_ns;
        if !(0..=i128::from(i64::MAX)).contains(&local) {
            return Err(Error::Invalid(
                "output timestamp outside engine clock range",
            ));
        }
        Ok(OutputTime {
            local_ns: local as u64,
            uncertainty_ns: self.uncertainty_ns,
        })
    }
}

/// Absolute sample timestamp from a stable sample origin. Use a cumulative sample
/// index across buffers; rounding each buffer's duration would accumulate error.
/// The origin is the first sample's OUTPUT time, including device latency once.
/// For an independent device oscillator, refresh the origin from its host timestamp
/// each callback; a nominal sample rate alone does not synchronize that oscillator.
pub fn sample_time(origin_ns: u64, sample_index: u64, sample_rate: u32) -> Result<u64, Error> {
    if sample_rate == 0 {
        return Err(Error::Invalid("zero sample rate"));
    }
    let delta = u128::from(sample_index) * 1_000_000_000 / u128::from(sample_rate);
    let delta = u64::try_from(delta).map_err(|_| Error::Invalid("sample timestamp overflow"))?;
    presentation_time(origin_ns, delta)
}
impl TimecodeSnapshot {
    /// One immutable trajectory across a block. Rate changes and scheduled
    /// discontinuities apply at each sample's timestamp, never at buffer receipt.
    pub fn evaluate_sample(
        &self,
        origin_ns: u64,
        sample_index: u64,
        sample_rate: u32,
    ) -> Result<Reading, Error> {
        Ok(self.evaluate(sample_time(origin_ns, sample_index, sample_rate)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FrameFormat, Position, Rate, SyncState,
        clock::ClockMapping,
        timeline::{Anchor, Scheduled, View},
    };
    #[test]
    fn clock_bridge_bounds_conversion_without_pairing_bias() {
        let b = ClockBridge::new(100, 10_000, 110).unwrap();
        let at = b.convert(10_100).unwrap();
        assert_eq!(at.local_ns, 205);
        assert!(at.uncertainty_ns >= 5.);
        assert!(b.convert(0).is_err());
        assert!(ClockBridge::new(110, 100, 100).is_err());
        assert!(ClockBridge::new(0, 0, u64::MAX).is_err());
    }
    #[test]
    fn audio_samples_keep_fractional_phase_across_arbitrary_buffers_and_controls() {
        let mut view = View {
            sync: SyncState::Synchronized,
            mapping: ClockMapping {
                last_sample_ns: 1,
                uncertainty_ns: 0.,
                ..Default::default()
            },
            ..Default::default()
        };
        view.timeline.format = FrameFormat::new(30000, 1001, true).unwrap();
        view.timeline.anchor.rate = Rate::NORMAL;
        view.timeline.scheduled_len = 1;
        view.timeline.scheduled[0] = Scheduled {
            discontinuity: 1,
            anchor: Anchor {
                time_ns: 1_000_000_000,
                position: Position::from_frames(90),
                rate: Rate::new(-1, 1).unwrap(),
            },
        };
        let snapshot = TimecodeSnapshot::from(view);
        for sample_rate in [44100, 48000, 96000] {
            let mut sample = 0;
            for size in [127, 512, 63, 2048].into_iter().cycle().take(150) {
                for index in sample..sample + size {
                    let ns = (u128::from(index) * 1_000_000_000 / u128::from(sample_rate)) as u64;
                    let got = snapshot.evaluate_sample(1, index, sample_rate).unwrap();
                    let expected = view.evaluate(1 + ns);
                    assert_eq!(got.position, expected.position);
                    assert_eq!(got.discontinuity, expected.discontinuity);
                }
                sample += size;
            }
        }
        assert!(sample_time(0, 0, 0).is_err());
        assert!(sample_time(i64::MAX as u64, 1, 48000).is_err());
        assert!(sample_time(0, u64::MAX, 1).is_err());
    }
}
