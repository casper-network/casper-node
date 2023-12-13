use core::fmt::{self, Display, Formatter};

use alloc::vec::Vec;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

/// An unbroken, inclusive range of blocks.
#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AvailableBlockRange {
    /// The inclusive lower bound of the range.
    low: u64,
    /// The inclusive upper bound of the range.
    high: u64,
}

impl AvailableBlockRange {
    /// An `AvailableRange` of [0, 0].
    pub const RANGE_0_0: AvailableBlockRange = AvailableBlockRange { low: 0, high: 0 };

    /// Constructs a new `AvailableBlockRange` with the given limits.
    pub fn new(low: u64, high: u64) -> Self {
        assert!(
            low <= high,
            "cannot construct available block range with low > high"
        );
        AvailableBlockRange { low, high }
    }

    /// Returns `true` if `height` is within the range.
    pub fn contains(&self, height: u64) -> bool {
        height >= self.low && height <= self.high
    }

    /// Returns the low value.
    pub fn low(&self) -> u64 {
        self.low
    }

    /// Returns the high value.
    pub fn high(&self) -> u64 {
        self.high
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let low = rng.gen::<u16>() as u64;
        let high = low + rng.gen::<u16>() as u64;
        Self { low, high }
    }
}

impl Display for AvailableBlockRange {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "available block range [{}, {}]",
            self.low, self.high
        )
    }
}

impl ToBytes for AvailableBlockRange {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.low.write_bytes(writer)?;
        self.high.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.low.serialized_length() + self.high.serialized_length()
    }
}

impl FromBytes for AvailableBlockRange {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (low, remainder) = u64::from_bytes(bytes)?;
        let (high, remainder) = u64::from_bytes(remainder)?;
        Ok((AvailableBlockRange { low, high }, remainder))
    }
}
