use datasize::DataSize;
#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::EraId;

use crate::types::{BlockHash, BlockHashAndHeight};

#[derive(Clone, Copy, Debug, DataSize)]
pub(crate) struct BlockHashHeightAndEra {
    pub(crate) block_hash: BlockHash,
    pub(crate) block_height: u64,
    pub(crate) era_id: EraId,
}

impl BlockHashHeightAndEra {
    pub(crate) fn new(block_hash: BlockHash, block_height: u64, era_id: EraId) -> Self {
        BlockHashHeightAndEra {
            block_hash,
            block_height,
            era_id,
        }
    }

    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            block_hash: BlockHash::random(rng),
            block_height: rng.gen(),
            era_id: rng.gen(),
        }
    }
}

impl From<&BlockHashHeightAndEra> for BlockHashAndHeight {
    fn from(value: &BlockHashHeightAndEra) -> Self {
        BlockHashAndHeight {
            block_hash: value.block_hash,
            block_height: value.block_height,
        }
    }
}
