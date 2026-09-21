use crate::Error;
/// Exact frame rate; fields are private to preserve arithmetic invariants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameFormat {
    numerator: u32,
    denominator: u32,
    drop_frame: bool,
}
impl Default for FrameFormat {
    fn default() -> Self {
        Self::new(30, 1, false).unwrap()
    }
}
impl FrameFormat {
    pub fn new(numerator: u32, denominator: u32, drop_frame: bool) -> Result<Self, Error> {
        if !matches!(
            (numerator, denominator),
            (24000, 1001)
                | (24, 1)
                | (25, 1)
                | (30000, 1001)
                | (30, 1)
                | (48000, 1001)
                | (48, 1)
                | (50, 1)
                | (60000, 1001)
                | (60, 1)
        ) || (drop_frame && !matches!((numerator, denominator), (30000, 1001) | (60000, 1001)))
        {
            return Err(Error::Invalid("frame format"));
        }
        Ok(Self {
            numerator,
            denominator,
            drop_frame,
        })
    }
    pub fn numerator(self) -> u32 {
        self.numerator
    }
    pub fn denominator(self) -> u32 {
        self.denominator
    }
    pub fn drop_frame(self) -> bool {
        self.drop_frame
    }
    pub fn fps(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
    pub fn nominal(self) -> u32 {
        self.numerator.div_ceil(self.denominator)
    }
    pub fn frames_per_day(self) -> i64 {
        let nominal = self.nominal() as i64;
        86400 * nominal
            - if self.drop_frame {
                nominal / 15 * 1296
            } else {
                0
            }
    }
    /// Display wraps at 24 hours. Negative positions use Euclidean wrapping.
    pub fn label(self, position: Position) -> Label {
        let n = self.nominal() as i64;
        let mut frame = position.frames.rem_euclid(self.frames_per_day());
        if self.drop_frame {
            let d = n / 15;
            let ten = n * 600 - d * 9;
            let rem = frame % ten;
            frame += d * 9 * (frame / ten)
                + if rem >= d {
                    d * ((rem - d) / (n * 60 - d))
                } else {
                    0
                };
        }
        Label {
            hours: (frame / (n * 3600)) as u8,
            minutes: (frame / (n * 60) % 60) as u8,
            seconds: (frame / n % 60) as u8,
            frames: (frame % n) as u8,
            drop_frame: self.drop_frame,
        }
    }
    pub fn position(
        self,
        hours: u8,
        minutes: u8,
        seconds: u8,
        frames: u8,
    ) -> Result<Position, Error> {
        let n = self.nominal() as i64;
        let d = if self.drop_frame { n / 15 } else { 0 };
        if hours >= 24
            || minutes >= 60
            || seconds >= 60
            || frames as i64 >= n
            || (!minutes.is_multiple_of(10) && seconds == 0 && (frames as i64) < d)
        {
            return Err(Error::Invalid("timecode label"));
        }
        let mins = hours as i64 * 60 + minutes as i64;
        Ok(Position::from_frames(
            (mins * 60 + seconds as i64) * n + frames as i64 - d * (mins - mins / 10),
        ))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
    pub drop_frame: bool,
}
impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}{}{:02}",
            self.hours,
            self.minutes,
            self.seconds,
            if self.drop_frame { ';' } else { ':' },
            self.frames
        )
    }
}
/// Signed unwrapped frames with Q32 subframes; -0.5 is (-1, 0x80000000).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub frames: i64,
    pub subframe: u32,
}
impl Position {
    pub const ZERO: Self = Self::from_frames(0);
    pub const fn from_frames(frames: i64) -> Self {
        Self {
            frames,
            subframe: 0,
        }
    }
    pub fn fixed(self) -> i128 {
        (self.frames as i128) * (1i128 << 32) + self.subframe as i128
    }
    pub fn from_fixed(value: i128) -> Self {
        let v = value.clamp(
            (i64::MIN as i128) << 32,
            ((i64::MAX as i128) << 32) + u32::MAX as i128,
        );
        Self {
            frames: (v >> 32) as i64,
            subframe: v as u32,
        }
    }
    pub fn as_frames(self) -> f64 {
        self.frames as f64 + self.subframe as f64 / 4294967296.0
    }
    pub fn advance(self, elapsed_ns: i64, format: FrameFormat, rate: Rate) -> Self {
        // All products fit i128 under the validated rate and format bounds.
        let num = elapsed_ns as i128 * format.numerator as i128 * rate.numerator as i128;
        let den = 1_000_000_000i128 * format.denominator as i128 * rate.denominator as i128;
        let delta = (num / den) * (1i128 << 32) + (num % den) * (1i128 << 32) / den;
        Self::from_fixed(self.fixed().saturating_add(delta))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    numerator: i32,
    denominator: u32,
}
impl Rate {
    pub const PAUSED: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    pub const NORMAL: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    pub fn new(numerator: i32, denominator: u32) -> Result<Self, Error> {
        if denominator == 0
            || denominator > 1_000_000
            || numerator.unsigned_abs() > 1_000_000
            || numerator.unsigned_abs() as u64 > denominator as u64 * 64
        {
            return Err(Error::Invalid("rate (maximum magnitude 64)"));
        }
        let mut a = numerator.unsigned_abs();
        let mut b = denominator;
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        Ok(Self {
            numerator: numerator / (a as i32),
            denominator: denominator / a,
        })
    }
    pub fn numerator(self) -> i32 {
        self.numerator
    }
    pub fn denominator(self) -> u32 {
        self.denominator
    }
    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rational_reverse_subframe() {
        let f = FrameFormat::new(30000, 1001, false).unwrap();
        assert_eq!(
            Position::ZERO.advance(1_001_000_000, f, Rate::NORMAL),
            Position::from_frames(30)
        );
        assert_eq!(
            Position::ZERO.advance(
                -500_000_000,
                FrameFormat::default(),
                Rate::new(1, 30).unwrap()
            ),
            Position {
                frames: -1,
                subframe: 1 << 31
            }
        );
        assert!(
            Position::ZERO
                .advance(i64::MAX, f, Rate::new(-64, 1).unwrap())
                .frames
                < 0
        );
        assert!(Rate::new(i32::MIN, 1).is_err());
    }
    #[test]
    fn drop_frame_roundtrip_day() {
        for n in [30000, 60000] {
            let f = FrameFormat::new(n, 1001, true).unwrap();
            for frame in 0..f.frames_per_day() {
                let l = f.label(Position::from_frames(frame));
                assert_eq!(
                    f.position(l.hours, l.minutes, l.seconds, l.frames)
                        .unwrap()
                        .frames,
                    frame
                );
            }
            assert_eq!(
                f.label(Position::from_frames(f.frames_per_day()))
                    .to_string(),
                "00:00:00;00"
            );
            assert!(f.position(0, 1, 0, 0).is_err());
            assert_eq!(f.label(Position::from_frames(-1)).hours, 23);
        }
    }
    #[test]
    fn all_formats_and_midnight() {
        for (n, d) in [
            (24000, 1001),
            (24, 1),
            (25, 1),
            (30000, 1001),
            (30, 1),
            (48000, 1001),
            (48, 1),
            (50, 1),
            (60000, 1001),
            (60, 1),
        ] {
            let f = FrameFormat::new(n, d, false).unwrap();
            assert_eq!(f.label(Position::from_frames(f.frames_per_day())).hours, 0);
            assert_eq!(f.label(Position::from_frames(-1)).hours, 23);
        }
    }
}
