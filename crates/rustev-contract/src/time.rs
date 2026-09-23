//! Time as a value (spec 002, 3.4.4). Nothing in Rustev's contract or core
//! reads a clock; the evaluation time is supplied.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// The largest timestamp: 9999-12-31T23:59:59.999Z.
pub const MAX_TIMESTAMP_MS: i64 = 253_402_300_799_999;

/// Milliseconds since the Unix epoch, UTC, in `0..=MAX_TIMESTAMP_MS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

/// A non-negative span in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DurationMs(pub u64);

/// A timestamp outside the representable range, or an arithmetic result
/// that would leave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRangeError;

impl fmt::Display for TimeRangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timestamp outside 0..={MAX_TIMESTAMP_MS} ms")
    }
}

impl std::error::Error for TimeRangeError {}

impl Timestamp {
    /// A validated timestamp.
    pub fn from_ms(ms: i64) -> Result<Self, TimeRangeError> {
        if (0..=MAX_TIMESTAMP_MS).contains(&ms) {
            Ok(Self(ms))
        } else {
            Err(TimeRangeError)
        }
    }

    pub fn as_ms(self) -> i64 {
        self.0
    }

    /// `self - earlier`, refused when negative.
    pub fn since(self, earlier: Timestamp) -> Result<DurationMs, TimeRangeError> {
        if self.0 < earlier.0 {
            return Err(TimeRangeError);
        }
        // Both are in range, so the difference fits.
        Ok(DurationMs((self.0 - earlier.0) as u64))
    }

    pub fn checked_add(self, d: DurationMs) -> Result<Timestamp, TimeRangeError> {
        let d = i64::try_from(d.0).map_err(|_| TimeRangeError)?;
        Timestamp::from_ms(self.0.checked_add(d).ok_or(TimeRangeError)?)
    }

    pub fn checked_sub(self, d: DurationMs) -> Result<Timestamp, TimeRangeError> {
        let d = i64::try_from(d.0).map_err(|_| TimeRangeError)?;
        Timestamp::from_ms(self.0.checked_sub(d).ok_or(TimeRangeError)?)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let ms = i64::deserialize(d)?;
        Timestamp::from_ms(ms).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_is_enforced_at_both_ends() {
        assert!(Timestamp::from_ms(0).is_ok());
        assert!(Timestamp::from_ms(MAX_TIMESTAMP_MS).is_ok());
        assert!(Timestamp::from_ms(-1).is_err());
        assert!(Timestamp::from_ms(MAX_TIMESTAMP_MS + 1).is_err());
        assert!(serde_json::from_str::<Timestamp>("-1").is_err());
        assert!(serde_json::from_str::<Timestamp>("1.5").is_err());
    }

    #[test]
    fn arithmetic_is_checked() {
        let t = Timestamp::from_ms(1_000).unwrap();
        assert_eq!(
            t.since(Timestamp::from_ms(400).unwrap()),
            Ok(DurationMs(600))
        );
        assert!(t.since(Timestamp::from_ms(1_001).unwrap()).is_err());
        assert!(t.checked_sub(DurationMs(1_001)).is_err());
        assert!(
            Timestamp::from_ms(MAX_TIMESTAMP_MS)
                .unwrap()
                .checked_add(DurationMs(1))
                .is_err()
        );
        assert!(t.checked_add(DurationMs(u64::MAX)).is_err());
    }
}
