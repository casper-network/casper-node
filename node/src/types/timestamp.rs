// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    fmt::{self, Display, Formatter},
    ops::{Add, AddAssign, Div, Mul, Rem},
    str::FromStr,
    time::{Duration, SystemTime},
};

use datasize::DataSize;
use derive_more::{Add, AddAssign, From, Shl, Shr, Sub, SubAssign};
use humantime::{DurationError, TimestampError};
use once_cell::sync::Lazy;
#[cfg(test)]
use rand::Rng;
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use crate::rpcs::docs::DocExample;

#[cfg(test)]
use crate::testing::TestRng;

static TIMESTAMP_EXAMPLE: Lazy<Timestamp> = Lazy::new(|| {
    let example_str: &str = "2020-11-17T00:39:24.072Z";
    Timestamp::from_str(example_str).unwrap()
});

/// A timestamp type, representing a concrete moment in time.
#[derive(
    DataSize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Shr, Shl, JsonSchema,
)]
#[serde(deny_unknown_fields)]
#[schemars(with = "String", description = "Timestamp formatted as per RFC 3339")]
pub struct Timestamp(u64);

impl Timestamp {
    /// Returns the timestamp of the current moment.
    pub fn now() -> Self {
        let millis = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_millis() as u64;
        Timestamp(millis)
    }

    /// Returns the time that has elapsed since this timestamp.
    pub fn elapsed(&self) -> TimeDiff {
        TimeDiff(Timestamp::now().0.saturating_sub(self.0))
    }

    /// Returns a zero timestamp.
    pub fn zero() -> Self {
        Timestamp(0)
    }

    /// Returns the timestamp as the number of milliseconds since the Unix epoch
    pub fn millis(&self) -> u64 {
        self.0
    }

    /// Returns the difference between `self` and `other`, or `0` if `self` is earlier than `other`.
    pub fn saturating_diff(self, other: Timestamp) -> TimeDiff {
        TimeDiff(self.0.saturating_sub(other.0))
    }

    /// Returns the difference between `self` and `other`, or `0` if that would be before the epoch.
    pub fn saturating_sub(self, other: TimeDiff) -> Timestamp {
        Timestamp(self.0.saturating_sub(other.0))
    }

    /// Returns the sum of `self` and `other`, or the maximum possible value if that would be
    /// exceeded.
    pub fn saturating_add(self, other: TimeDiff) -> Timestamp {
        Timestamp(self.0.saturating_add(other.0))
    }

    /// Returns the number of trailing zeros in the number of milliseconds since the epoch.
    pub fn trailing_zeros(&self) -> u8 {
        self.0.trailing_zeros() as u8
    }
}

#[cfg(test)]
impl Timestamp {
    /// Generates a random instance using a `TestRng`.
    pub fn random(rng: &mut TestRng) -> Self {
        Timestamp(1_596_763_000_000 + rng.gen_range(200_000..1_000_000))
    }

    /// Checked subtraction for timestamps
    pub fn checked_sub(self, other: TimeDiff) -> Option<Timestamp> {
        self.0.checked_sub(other.0).map(Timestamp)
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match SystemTime::UNIX_EPOCH.checked_add(Duration::from_millis(self.0)) {
            Some(system_time) => write!(f, "{}", humantime::format_rfc3339_millis(system_time)),
            None => write!(f, "invalid Timestamp: {} ms after the Unix epoch", self.0),
        }
    }
}

impl DocExample for Timestamp {
    fn doc_example() -> &'static Self {
        &*TIMESTAMP_EXAMPLE
    }
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let system_time = humantime::parse_rfc3339_weak(value)?;
        let inner = system_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| TimestampError::OutOfRange)?
            .as_millis() as u64;
        Ok(Timestamp(inner))
    }
}

impl Add<TimeDiff> for Timestamp {
    type Output = Timestamp;

    fn add(self, diff: TimeDiff) -> Timestamp {
        Timestamp(self.0 + diff.0)
    }
}

impl AddAssign<TimeDiff> for Timestamp {
    fn add_assign(&mut self, rhs: TimeDiff) {
        self.0 += rhs.0;
    }
}

#[cfg(test)]
impl std::ops::Sub<TimeDiff> for Timestamp {
    type Output = Timestamp;

    fn sub(self, diff: TimeDiff) -> Timestamp {
        Timestamp(self.0 - diff.0)
    }
}

impl Rem<TimeDiff> for Timestamp {
    type Output = TimeDiff;

    fn rem(self, diff: TimeDiff) -> TimeDiff {
        TimeDiff(self.0 % diff.0)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let value_as_string = String::deserialize(deserializer)?;
            Timestamp::from_str(&value_as_string).map_err(SerdeError::custom)
        } else {
            let inner = u64::deserialize(deserializer)?;
            Ok(Timestamp(inner))
        }
    }
}

impl ToBytes for Timestamp {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Timestamp {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        u64::from_bytes(bytes).map(|(inner, remainder)| (Timestamp(inner), remainder))
    }
}

impl From<u64> for Timestamp {
    fn from(milliseconds_since_epoch: u64) -> Timestamp {
        Timestamp(milliseconds_since_epoch)
    }
}

/// A time difference between two timestamps.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    DataSize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Add,
    AddAssign,
    Sub,
    SubAssign,
    From,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
#[schemars(with = "String", description = "Human-readable duration.")]
pub struct TimeDiff(u64);

impl Display for TimeDiff {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", humantime::format_duration(Duration::from(*self)))
    }
}

impl FromStr for TimeDiff {
    type Err = DurationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let inner = humantime::parse_duration(value)?.as_millis() as u64;
        Ok(TimeDiff(inner))
    }
}

impl TimeDiff {
    /// Returns the time difference as the number of milliseconds since the Unix epoch
    pub fn millis(&self) -> u64 {
        self.0
    }

    /// Creates a new time difference from seconds.
    pub const fn from_seconds(seconds: u32) -> Self {
        TimeDiff(seconds as u64 * 1_000)
    }

    /// Returns the product, or `TimeDiff(u64::MAX)` if it would overflow.
    pub fn saturating_mul(self, rhs: u64) -> Self {
        TimeDiff(self.0.saturating_mul(rhs))
    }
}

impl Mul<u64> for TimeDiff {
    type Output = TimeDiff;

    fn mul(self, rhs: u64) -> TimeDiff {
        TimeDiff(self.0 * rhs)
    }
}

impl Div<u64> for TimeDiff {
    type Output = TimeDiff;

    fn div(self, rhs: u64) -> TimeDiff {
        TimeDiff(self.0 / rhs)
    }
}

impl Div<TimeDiff> for TimeDiff {
    type Output = u64;

    fn div(self, rhs: TimeDiff) -> u64 {
        self.0 / rhs.0
    }
}

impl From<TimeDiff> for Duration {
    fn from(diff: TimeDiff) -> Duration {
        Duration::from_millis(diff.0)
    }
}

impl Serialize for TimeDiff {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for TimeDiff {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let value_as_string = String::deserialize(deserializer)?;
            TimeDiff::from_str(&value_as_string).map_err(SerdeError::custom)
        } else {
            let inner = u64::deserialize(deserializer)?;
            Ok(TimeDiff(inner))
        }
    }
}

impl ToBytes for TimeDiff {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for TimeDiff {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        u64::from_bytes(bytes).map(|(inner, remainder)| (TimeDiff(inner), remainder))
    }
}

impl From<Duration> for TimeDiff {
    fn from(duration: Duration) -> TimeDiff {
        TimeDiff(duration.as_millis() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_serialization_roundtrip() {
        let timestamp = Timestamp::now();

        let timestamp_as_string = timestamp.to_string();
        assert_eq!(
            timestamp,
            Timestamp::from_str(&timestamp_as_string).unwrap()
        );

        let serialized_json = serde_json::to_string(&timestamp).unwrap();
        assert_eq!(timestamp, serde_json::from_str(&serialized_json).unwrap());

        let serialized_bincode = bincode::serialize(&timestamp).unwrap();
        assert_eq!(
            timestamp,
            bincode::deserialize(&serialized_bincode).unwrap()
        );

        bytesrepr::test_serialization_roundtrip(&timestamp);
    }

    #[test]
    fn timediff_serialization_roundtrip() {
        let mut rng = crate::new_rng();
        let timediff = TimeDiff(rng.gen());

        let timediff_as_string = timediff.to_string();
        assert_eq!(timediff, TimeDiff::from_str(&timediff_as_string).unwrap());

        let serialized_json = serde_json::to_string(&timediff).unwrap();
        assert_eq!(timediff, serde_json::from_str(&serialized_json).unwrap());

        let serialized_bincode = bincode::serialize(&timediff).unwrap();
        assert_eq!(timediff, bincode::deserialize(&serialized_bincode).unwrap());

        bytesrepr::test_serialization_roundtrip(&timediff);
    }
}
