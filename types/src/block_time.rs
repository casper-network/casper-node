use alloc::vec::Vec;

use crate::bytesrepr::{Error, FromBytes, ToBytes, U64_SERIALIZED_LENGTH};

/// The number of bytes in a serialized [`BlockTime`].
pub const BLOCKTIME_SERIALIZED_LENGTH: usize = U64_SERIALIZED_LENGTH;

/// A newtype wrapping a [`u64`] which represents the block time.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd)]
pub struct BlockTime(u64);

impl BlockTime {
    /// Constructs a `BlockTime`.
    pub fn new(value: u64) -> Self {
        BlockTime(value)
    }

    /// Saturating integer subtraction. Computes `self - other`, saturating at `0` instead of
    /// overflowing.
    #[must_use]
    pub fn saturating_sub(self, other: BlockTime) -> Self {
        BlockTime(self.0.saturating_sub(other.0))
    }
}

impl From<BlockTime> for u64 {
    fn from(blocktime: BlockTime) -> Self {
        blocktime.0
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
