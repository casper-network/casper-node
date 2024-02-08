use alloc::vec::Vec;
use core::{
    fmt::{self, Display, Formatter},
    ops::{Add, AddAssign, Div, Mul, Rem, Shl, Shr, Sub, SubAssign},
    time::Duration,
};
#[cfg(any(feature = "std", test))]
use std::{str::FromStr, time::SystemTime};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use humantime::{DurationError, TimestampError};
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::bytesrepr::{self, FromBytes, ToBytes};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// Example timestamp equal to 2020-11-17T00:39:24.072Z.
#[cfg(feature = "json-schema")]
const TIMESTAMP: Timestamp = Timestamp(1_605_573_564_072);

/// A timestamp type, representing a concrete moment in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Timestamp formatted as per RFC 3339")
)]
pub struct Timestamp(#[cfg_attr(feature = "json-schema", schemars(with = "String"))] u64);

impl Timestamp {
    /// The maximum value a timestamp can have.
    pub const MAX: Timestamp = Timestamp(u64::MAX);

    #[cfg(any(feature = "std", test))]
    /// Returns the timestamp of the current moment.
    pub fn now() -> Self {
        let millis = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_millis() as u64;
        Timestamp(millis)
    }

    #[cfg(any(feature = "std", test))]
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
    #[must_use]
    pub fn saturating_sub(self, other: TimeDiff) -> Timestamp {
        Timestamp(self.0.saturating_sub(other.0))
    }

    /// Returns the sum of `self` and `other`, or the maximum possible value if that would be
    /// exceeded.
    #[must_use]
    pub fn saturating_add(self, other: TimeDiff) -> Timestamp {
        Timestamp(self.0.saturating_add(other.0))
    }

    /// Returns the number of trailing zeros in the number of milliseconds since the epoch.
    pub fn trailing_zeros(&self) -> u8 {
        self.0.trailing_zeros() as u8
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &TIMESTAMP
    }

    /// Returns a random `Timestamp`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        Timestamp(1_596_763_000_000 + rng.gen_range(200_000..1_000_000))
    }

    /// Checked subtraction for timestamps
    #[cfg(any(feature = "testing", test))]
    pub fn checked_sub(self, other: TimeDiff) -> Option<Timestamp> {
        self.0.checked_sub(other.0).map(Timestamp)
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        #[cfg(any(feature = "std", test))]
        return match SystemTime::UNIX_EPOCH.checked_add(Duration::from_millis(self.0)) {
            Some(system_time) => write!(f, "{}", humantime::format_rfc3339_millis(system_time))
                .or_else(|e| write!(f, "Invalid timestamp: {}: {}", e, self.0)),
            None => write!(f, "invalid Timestamp: {} ms after the Unix epoch", self.0),
        };

        #[cfg(not(any(feature = "std", test)))]
        write!(f, "timestamp({}ms)", self.0)
    }
}

#[cfg(any(feature = "std", test))]
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

#[cfg(any(feature = "testing", test))]
impl Sub<TimeDiff> for Timestamp {
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

impl<T> Shl<T> for Timestamp
where
    u64: Shl<T, Output = u64>,
{
    type Output = Timestamp;

    fn shl(self, rhs: T) -> Timestamp {
        Timestamp(self.0 << rhs)
    }
}

impl<T> Shr<T> for Timestamp
where
    u64: Shr<T, Output = u64>,
{
    type Output = Timestamp;

    fn shr(self, rhs: T) -> Timestamp {
        Timestamp(self.0 >> rhs)
    }
}

#[cfg(any(feature = "std", test))]
impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

#[cfg(any(feature = "std", test))]
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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Human-readable duration.")
)]
pub struct TimeDiff(#[cfg_attr(feature = "json-schema", schemars(with = "String"))] u64);

impl Display for TimeDiff {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        #[cfg(any(feature = "std", test))]
        return write!(f, "{}", humantime::format_duration(Duration::from(*self)));

        #[cfg(not(any(feature = "std", test)))]
        write!(f, "time diff({}ms)", self.0)
    }
}

#[cfg(any(feature = "std", test))]
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

    /// Creates a new time difference from milliseconds.
    pub const fn from_millis(millis: u64) -> Self {
        TimeDiff(millis)
    }

    /// Returns the product, or `TimeDiff(u64::MAX)` if it would overflow.
    #[must_use]
    pub fn saturating_mul(self, rhs: u64) -> Self {
        TimeDiff(self.0.saturating_mul(rhs))
    }
}

impl Add<TimeDiff> for TimeDiff {
    type Output = TimeDiff;

    fn add(self, rhs: TimeDiff) -> TimeDiff {
        TimeDiff(self.0 + rhs.0)
    }
}

impl AddAssign<TimeDiff> for TimeDiff {
    fn add_assign(&mut self, rhs: TimeDiff) {
        self.0 += rhs.0;
    }
}

impl Sub<TimeDiff> for TimeDiff {
    type Output = TimeDiff;

    fn sub(self, rhs: TimeDiff) -> TimeDiff {
        TimeDiff(self.0 - rhs.0)
    }
}

impl SubAssign<TimeDiff> for TimeDiff {
    fn sub_assign(&mut self, rhs: TimeDiff) {
        self.0 -= rhs.0;
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

#[cfg(any(feature = "std", test))]
impl Serialize for TimeDiff {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_string().serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

#[cfg(any(feature = "std", test))]
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

/// A module for the `[serde(with = serde_option_time_diff)]` attribute, to serialize and
/// deserialize `Option<TimeDiff>` treating `None` as 0.
#[cfg(any(feature = "std", test))]
pub mod serde_option_time_diff {
    use super::*;

    /// Serializes an `Option<TimeDiff>`, using `0` if the value is `None`.
    pub fn serialize<S: Serializer>(
        maybe_td: &Option<TimeDiff>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        maybe_td
            .unwrap_or_else(|| TimeDiff::from_millis(0))
            .serialize(serializer)
    }

    /// Deserializes an `Option<TimeDiff>`, returning `None` if the value is `0`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<TimeDiff>, D::Error> {
        let td = TimeDiff::deserialize(deserializer)?;
        if td.0 == 0 {
            Ok(None)
        } else {
            Ok(Some(td))
        }
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
        let mut rng = TestRng::new();
        let timediff = TimeDiff(rng.gen());

        let timediff_as_string = timediff.to_string();
        assert_eq!(timediff, TimeDiff::from_str(&timediff_as_string).unwrap());

        let serialized_json = serde_json::to_string(&timediff).unwrap();
        assert_eq!(timediff, serde_json::from_str(&serialized_json).unwrap());

        let serialized_bincode = bincode::serialize(&timediff).unwrap();
        assert_eq!(timediff, bincode::deserialize(&serialized_bincode).unwrap());

        bytesrepr::test_serialization_roundtrip(&timediff);
    }

    #[test]
    fn does_not_crash_for_big_timestamp_value() {
        assert!(Timestamp::MAX.to_string().starts_with("Invalid timestamp:"));
    }
}
