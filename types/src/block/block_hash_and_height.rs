use core::fmt::{self, Display, Formatter};

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
