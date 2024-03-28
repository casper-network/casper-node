use alloc::vec::Vec;

use crate::{
    bytesrepr::{Error, FromBytes, ToBytes, U64_SERIALIZED_LENGTH},
    CLType, CLTyped, TimeDiff, Timestamp,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The number of bytes in a serialized [`BlockTime`].
pub const BLOCKTIME_SERIALIZED_LENGTH: usize = U64_SERIALIZED_LENGTH;

/// Holds epoch type.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HoldsEpoch(Option<u64>);

impl HoldsEpoch {
    /// No epoch is applicable.
    pub const NOT_APPLICABLE: HoldsEpoch = HoldsEpoch(None);

    /// Instance from block time.
    pub fn from_block_time(block_time: BlockTime, hold_internal: TimeDiff) -> Self {
        HoldsEpoch(Some(
            block_time.value().saturating_sub(hold_internal.millis()),
        ))
    }

    /// Instance from timestamp.
    pub fn from_timestamp(timestamp: Timestamp, hold_internal: TimeDiff) -> Self {
        HoldsEpoch(Some(
            timestamp.millis().saturating_sub(hold_internal.millis()),
        ))
    }

    /// Instance from milliseconds.
    pub fn from_millis(timestamp_millis: u64, hold_internal_millis: u64) -> Self {
        HoldsEpoch(Some(timestamp_millis.saturating_sub(hold_internal_millis)))
    }

    /// Returns the inner value.
    pub fn value(&self) -> Option<u64> {
        self.0
    }
}

/// A newtype wrapping a [`u64`] which represents the block time.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
pub struct BlockTime(u64);

impl BlockTime {
    /// Constructs a `BlockTime`.
    pub const fn new(value: u64) -> Self {
        BlockTime(value)
    }

    /// Saturating integer subtraction. Computes `self - other`, saturating at `0` instead of
    /// overflowing.
    #[must_use]
    pub fn saturating_sub(self, other: BlockTime) -> Self {
        BlockTime(self.0.saturating_sub(other.0))
    }

    /// Returns inner value.
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl From<BlockTime> for u64 {
    fn from(blocktime: BlockTime) -> Self {
        blocktime.0
    }
}

impl From<BlockTime> for Timestamp {
    fn from(value: BlockTime) -> Self {
        Timestamp::from(value.0)
    }
}

impl From<u64> for BlockTime {
    fn from(value: u64) -> Self {
        BlockTime(value)
    }
}

impl From<Timestamp> for BlockTime {
    fn from(value: Timestamp) -> Self {
        BlockTime(value.millis())
    }
}

impl ToBytes for BlockTime {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        BLOCKTIME_SERIALIZED_LENGTH
    }
}

impl FromBytes for BlockTime {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (time, rem) = FromBytes::from_bytes(bytes)?;
        Ok((BlockTime::new(time), rem))
    }
}

impl CLTyped for BlockTime {
    fn cl_type() -> CLType {
        CLType::U64
    }
}
