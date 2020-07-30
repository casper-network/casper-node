use std::{
    fmt::{self, Display},
    num::ParseIntError,
    ops::{Add, Div, Mul, Rem, Sub},
    str::FromStr,
    time::{Duration, SystemTime},
};

use derive_more::{Add, AddAssign, From, Shl, Shr, Sub, SubAssign};
use serde::{Deserialize, Serialize};

/// A timestamp type, representing a concrete moment in time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash, Shr, Shl,
)]
pub struct Timestamp(u64);

/// A time difference between two timestamps.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Add, AddAssign, Sub, SubAssign, From,
)]
pub struct TimeDiff(u64);

impl Timestamp {
    /// Returns the timestamp of the current moment
    pub fn now() -> Self {
        let millis = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_millis() as u64;
        Timestamp(millis)
    }

    /// Returns a zero timestamp
    pub fn zero() -> Self {
        Timestamp(0)
    }

    /// Returns the timestamp as the number of milliseconds since the Unix epoch
    pub fn millis(&self) -> u64 {
        self.0
    }

    /// Returns the difference between `self` and `other`, or `0` if `self` is earlier than `other`.
    pub fn saturating_sub(self, other: Timestamp) -> TimeDiff {
        TimeDiff(self.0.saturating_sub(other.0))
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Timestamp {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u64::from_str(s).map(Timestamp)
    }
}

impl Sub<Timestamp> for Timestamp {
    type Output = TimeDiff;

    fn sub(self, other: Timestamp) -> TimeDiff {
        TimeDiff(self.0 - other.0)
    }
}

impl Add<TimeDiff> for Timestamp {
    type Output = Timestamp;

    fn add(self, diff: TimeDiff) -> Timestamp {
        Timestamp(self.0 + diff.0)
    }
}

impl Sub<TimeDiff> for Timestamp {
    type Output = Timestamp;

    fn sub(self, diff: TimeDiff) -> Timestamp {
        Timestamp(self.0 - diff.0)
    }
}

impl Div<TimeDiff> for Timestamp {
    type Output = u64;

    fn div(self, rhs: TimeDiff) -> u64 {
        self.0 / rhs.0
    }
}

impl Rem<TimeDiff> for Timestamp {
    type Output = TimeDiff;

    fn rem(self, diff: TimeDiff) -> TimeDiff {
        TimeDiff(self.0 % diff.0)
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

impl From<TimeDiff> for Duration {
    fn from(diff: TimeDiff) -> Duration {
        Duration::from_millis(diff.0)
    }
}

#[cfg(test)]
impl From<u64> for Timestamp {
    fn from(arg: u64) -> Timestamp {
        Timestamp(arg)
    }
}
