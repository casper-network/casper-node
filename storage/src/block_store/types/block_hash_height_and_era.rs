use datasize::DataSize;
#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    BlockHash, BlockHashAndHeight, EraId,
};

/// Aggregates block identifying information.
#[derive(Clone, Copy, Debug, DataSize)]
pub struct BlockHashHeightAndEra {
    /// Block hash.
    pub block_hash: BlockHash,
    /// Block height.
    pub block_height: u64,
    /// EraId
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

    /// Returns the block hash.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        Self {
            block_hash: BlockHash::random(rng),
            block_height: rng.gen(),
            era_id: EraId::random(rng),
        }
    }
}

impl From<BlockHashHeightAndEra> for BlockHashAndHeight {
    fn from(bhhe: BlockHashHeightAndEra) -> Self {
        BlockHashAndHeight::new(bhhe.block_hash, bhhe.block_height)
    }
}

impl ToBytes for BlockHashHeightAndEra {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.block_hash.to_bytes()?);
        buffer.extend(self.block_height.to_bytes()?);
        buffer.extend(self.era_id.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length()
            + self.block_height.serialized_length()
            + self.era_id.serialized_length()
    }
}

impl FromBytes for BlockHashHeightAndEra {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (block_height, remainder) = u64::from_bytes(remainder)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        Ok((
            BlockHashHeightAndEra {
                block_hash,
                block_height,
                era_id,
            },
            remainder,
        ))
    }
}
