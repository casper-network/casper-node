//! Minimal info about a `Block` needed to satisfy the node status request.

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Block, BlockHash, Digest, EraId, PublicKey, Timestamp,
};
use alloc::vec::Vec;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

#[cfg(test)]
use rand::Rng;

#[cfg(test)]
use crate::testing::TestRng;

/// Minimal info about a `Block` needed to satisfy the node status request.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[cfg_attr(any(feature = "std", test), serde(deny_unknown_fields))]
pub struct MinimalBlockInfo {
    hash: BlockHash,
    timestamp: Timestamp,
    era_id: EraId,
    height: u64,
    state_root_hash: Digest,
    creator: PublicKey,
}

impl MinimalBlockInfo {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self {
            hash: BlockHash::random(rng),
            timestamp: Timestamp::random(rng),
            era_id: EraId::random(rng),
            height: rng.gen(),
            state_root_hash: Digest::random(rng),
            creator: PublicKey::random(rng),
        }
    }
}

impl FromBytes for MinimalBlockInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (timestamp, remainder) = Timestamp::from_bytes(remainder)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (height, remainder) = u64::from_bytes(remainder)?;
        let (state_root_hash, remainder) = Digest::from_bytes(remainder)?;
        let (creator, remainder) = PublicKey::from_bytes(remainder)?;
        Ok((
            MinimalBlockInfo {
                hash,
                timestamp,
                era_id,
                height,
                state_root_hash,
                creator,
            },
            remainder,
        ))
    }
}

impl ToBytes for MinimalBlockInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.hash.write_bytes(writer)?;
        self.timestamp.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.height.write_bytes(writer)?;
        self.state_root_hash.write_bytes(writer)?;
        self.creator.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.timestamp.serialized_length()
            + self.era_id.serialized_length()
            + self.height.serialized_length()
            + self.state_root_hash.serialized_length()
            + self.creator.serialized_length()
    }
}

impl From<Block> for MinimalBlockInfo {
    fn from(block: Block) -> Self {
        let proposer = match &block {
            Block::V1(v1) => v1.proposer().clone(),
            Block::V2(v2) => v2.proposer().clone(),
        };

        MinimalBlockInfo {
            hash: *block.hash(),
            timestamp: block.timestamp(),
            era_id: block.era_id(),
            height: block.height(),
            state_root_hash: *block.state_root_hash(),
            creator: proposer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = MinimalBlockInfo::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
