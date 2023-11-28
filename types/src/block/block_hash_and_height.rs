use core::fmt::{self, Display, Formatter};

use alloc::vec::Vec;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::BlockHash;
#[cfg(doc)]
use super::BlockV2;
use crate::bytesrepr::{self, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

/// The block hash and height of a given block.
#[derive(
    Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug, Default,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct BlockHashAndHeight {
    /// The hash of the block.
    block_hash: BlockHash,
    /// The height of the block.
    block_height: u64,
}

impl BlockHashAndHeight {
    /// Constructs a new `BlockHashAndHeight`.
    pub fn new(block_hash: BlockHash, block_height: u64) -> Self {
        Self {
            block_hash,
            block_height,
        }
    }

    /// Returns the hash of the block.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    /// Returns the height of the block.
    pub fn block_height(&self) -> u64 {
        self.block_height
    }

    /// Returns a random `BlockHashAndHeight`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            block_hash: BlockHash::random(rng),
            block_height: rng.gen(),
        }
    }
}

impl Display for BlockHashAndHeight {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}, height {} ",
            self.block_hash, self.block_height
        )
    }
}

impl ToBytes for BlockHashAndHeight {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.block_hash.write_bytes(writer)?;
        self.block_height.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length() + self.block_height.serialized_length()
    }
}

impl FromBytes for BlockHashAndHeight {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (block_height, remainder) = u64::from_bytes(remainder)?;
        Ok((
            BlockHashAndHeight {
                block_hash,
                block_height,
            },
            remainder,
        ))
    }
}
