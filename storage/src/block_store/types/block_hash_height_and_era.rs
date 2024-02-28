use datasize::DataSize;
#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{BlockHash, EraId};

#[derive(Clone, Copy, Debug, DataSize)]
pub struct BlockHashHeightAndEra {
    pub block_hash: BlockHash,
    pub block_height: u64,
    pub era_id: EraId,
}

impl BlockHashHeightAndEra {
    /// Creates a new [`BlockHashHeightAndEra`] from parts.
    pub fn new(block_hash: BlockHash, block_height: u64, era_id: EraId) -> Self {
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
            era_id: EraId::random(rng),
        }
    }
}
