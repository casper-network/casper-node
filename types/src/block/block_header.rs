mod block_header_v1;
mod block_header_v2;

pub use block_header_v1::BlockHeaderV1;
pub use block_header_v2::BlockHeaderV2;

use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "std")]
use crate::ProtocolConfig;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockHash, Digest, EraEnd, EraId, ProtocolVersion, PublicKey, Timestamp, U512,
};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block header v1.
pub const BLOCK_HEADER_V1_TAG: u8 = 0;
/// Tag for block header v2.
pub const BLOCK_HEADER_V2_TAG: u8 = 1;

/// The versioned header portion of a block. It encapsulates different variants of the BlockHeader
/// struct.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum BlockHeader {
    /// The legacy, initial version of the header portion of a block.
    #[cfg_attr(any(feature = "std", test), serde(rename = "Version1"))]
    V1(BlockHeaderV1),
    /// The version 2 of the header portion of a block.
    #[cfg_attr(any(feature = "std", test), serde(rename = "Version2"))]
    V2(BlockHeaderV2),
}

impl BlockHeader {
    /// Returns the hash of this block header.
    pub fn block_hash(&self) -> BlockHash {
        match self {
            BlockHeader::V1(v1) => v1.block_hash(),
            BlockHeader::V2(v2) => v2.block_hash(),
        }
    }

    /// Returns the parent block's hash.
    pub fn parent_hash(&self) -> &BlockHash {
        match self {
            BlockHeader::V1(v1) => v1.parent_hash(),
            BlockHeader::V2(v2) => v2.parent_hash(),
        }
    }

    /// Returns the root hash of global state after the deploys in this block have been executed.
    pub fn state_root_hash(&self) -> &Digest {
        match self {
            BlockHeader::V1(v1) => v1.state_root_hash(),
            BlockHeader::V2(v2) => v2.state_root_hash(),
        }
    }

    /// Returns the hash of the block's body.
    pub fn body_hash(&self) -> &Digest {
        match self {
            BlockHeader::V1(v1) => v1.body_hash(),
            BlockHeader::V2(v2) => v2.body_hash(),
        }
    }

    /// Returns a random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        match self {
            BlockHeader::V1(v1) => v1.random_bit(),
            BlockHeader::V2(v2) => v2.random_bit(),
        }
    }

    /// Returns a seed needed for initializing a future era.
    pub fn accumulated_seed(&self) -> &Digest {
        match self {
            BlockHeader::V1(v1) => v1.accumulated_seed(),
            BlockHeader::V2(v2) => v2.accumulated_seed(),
        }
    }

    /// Returns the `EraEnd` of a block if it is a switch block.
    pub fn clone_era_end(&self) -> Option<EraEnd> {
        match self {
            BlockHeader::V1(v1) => v1.era_end().map(|ee| ee.clone().into()),
            BlockHeader::V2(v2) => v2.era_end().map(|ee| ee.clone().into()),
        }
    }

    /// Returns equivocators if the header is of a switch block.
    pub fn maybe_equivocators(&self) -> Option<&[PublicKey]> {
        match self {
            BlockHeader::V1(v1) => v1.era_end().map(|ee| ee.equivocators()),
            BlockHeader::V2(v2) => v2.era_end().map(|ee| ee.equivocators()),
        }
    }

    /// Returns equivocators if the header is of a switch block.
    pub fn maybe_inactive_validators(&self) -> Option<&[PublicKey]> {
        match self {
            BlockHeader::V1(v1) => v1.era_end().map(|ee| ee.inactive_validators()),
            BlockHeader::V2(v2) => v2.era_end().map(|ee| ee.inactive_validators()),
        }
    }

    /// Returns the timestamp from when the block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            BlockHeader::V1(v1) => v1.timestamp(),
            BlockHeader::V2(v2) => v2.timestamp(),
        }
    }

    /// Returns the era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        match self {
            BlockHeader::V1(v1) => v1.era_id(),
            BlockHeader::V2(v2) => v2.era_id(),
        }
    }

    /// Returns the era ID in which the next block would be created (i.e. this block's era ID, or
    /// its successor if this is a switch block).
    pub fn next_block_era_id(&self) -> EraId {
        match self {
            BlockHeader::V1(v1) => v1.next_block_era_id(),
            BlockHeader::V2(v2) => v2.next_block_era_id(),
        }
    }

    /// Returns the height of this block, i.e. the number of ancestors.
    pub fn height(&self) -> u64 {
        match self {
            BlockHeader::V1(v1) => v1.height(),
            BlockHeader::V2(v2) => v2.height(),
        }
    }

    /// Returns the protocol version of the network from when this block was created.
    pub fn protocol_version(&self) -> ProtocolVersion {
        match self {
            BlockHeader::V1(v1) => v1.protocol_version(),
            BlockHeader::V2(v2) => v2.protocol_version(),
        }
    }

    /// Returns `true` if this block is the last one in the current era.
    pub fn is_switch_block(&self) -> bool {
        match self {
            BlockHeader::V1(v1) => v1.is_switch_block(),
            BlockHeader::V2(v2) => v2.is_switch_block(),
        }
    }

    /// Returns the validators for the upcoming era and their respective weights (if this is a
    /// switch block).
    pub fn next_era_validator_weights(&self) -> Option<&BTreeMap<PublicKey, U512>> {
        match self {
            BlockHeader::V1(v1) => v1.next_era_validator_weights(),
            BlockHeader::V2(v2) => v2.next_era_validator_weights(),
        }
    }

    /// Returns `true` if this block is the Genesis block, i.e. has height 0 and era 0.
    pub fn is_genesis(&self) -> bool {
        match self {
            BlockHeader::V1(v1) => v1.is_genesis(),
            BlockHeader::V2(v2) => v2.is_genesis(),
        }
    }

    /// Returns `true` if this block belongs to the last block before the upgrade to the
    /// current protocol version.
    #[cfg(feature = "std")]
    pub fn is_last_block_before_activation(&self, protocol_config: &ProtocolConfig) -> bool {
        match self {
            BlockHeader::V1(v1) => v1.is_last_block_before_activation(protocol_config),
            BlockHeader::V2(v2) => v2.is_last_block_before_activation(protocol_config),
        }
    }

    // This method is not intended to be used by third party crates.
    //
    // Sets the block hash without recomputing it. Must only be called with the correct hash.
    #[doc(hidden)]
    #[cfg(any(feature = "once_cell", test))]
    pub fn set_block_hash(&self, block_hash: BlockHash) {
        match self {
            BlockHeader::V1(v1) => v1.set_block_hash(block_hash),
            BlockHeader::V2(v2) => v2.set_block_hash(block_hash),
        }
    }
}

impl Display for BlockHeader {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            BlockHeader::V1(v1) => Display::fmt(&v1, formatter),
            BlockHeader::V2(v2) => Display::fmt(&v2, formatter),
        }
    }
}

impl From<BlockHeaderV1> for BlockHeader {
    fn from(header: BlockHeaderV1) -> Self {
        BlockHeader::V1(header)
    }
}

impl From<BlockHeaderV2> for BlockHeader {
    fn from(header: BlockHeaderV2) -> Self {
        BlockHeader::V2(header)
    }
}

impl ToBytes for BlockHeader {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            BlockHeader::V1(v1) => {
                buffer.insert(0, BLOCK_HEADER_V1_TAG);
                buffer.extend(v1.to_bytes()?);
            }
            BlockHeader::V2(v2) => {
                buffer.insert(0, BLOCK_HEADER_V2_TAG);
                buffer.extend(v2.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                BlockHeader::V1(v1) => v1.serialized_length(),
                BlockHeader::V2(v2) => v2.serialized_length(),
            }
    }
}

impl FromBytes for BlockHeader {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_HEADER_V1_TAG => {
                let (header, remainder): (BlockHeaderV1, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V1(header), remainder))
            }
            BLOCK_HEADER_V2_TAG => {
                let (header, remainder): (BlockHeaderV2, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V2(header), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, testing::TestRng, TestBlockBuilder, TestBlockV1Builder};

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let block_header_v1 = TestBlockV1Builder::new()
            .build_versioned(rng)
            .clone_header();
        bytesrepr::test_serialization_roundtrip(&block_header_v1);

        let block_header_v2 = TestBlockBuilder::new().build_versioned(rng).clone_header();
        bytesrepr::test_serialization_roundtrip(&block_header_v2);
    }
}
