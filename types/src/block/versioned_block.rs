use alloc::{boxed::Box, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Block, BlockHash, BlockHeader, BlockValidationError, DeployHash, Digest, EraId,
    ProtocolVersion, Timestamp, VersionedBlockBody,
};

use super::{block_v1::BlockV1, block_v2::BlockV2};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block body v1.
pub const BLOCK_V1_TAG: u8 = 0;
/// Tag for block body v2.
pub const BLOCK_V2_TAG: u8 = 1;

/// A block. It encapsulates different variants of the `BlockVx`.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(any(feature = "std", test), derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionedBlock {
    /// The legacy, initial version of the block.
    V1(BlockV1),
    /// The version 2 of the block.
    V2(BlockV2),
}

impl VersionedBlock {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn new_from_header_and_versioned_body(
        header: BlockHeader,
        versioned_block_body: VersionedBlockBody,
    ) -> Result<Self, Box<BlockValidationError>> {
        let hash = header.block_hash();
        let block = match versioned_block_body {
            VersionedBlockBody::V1(v1) => VersionedBlock::V1(BlockV1 {
                hash,
                header,
                body: v1,
            }),
            VersionedBlockBody::V2(v2) => VersionedBlock::V2(Block {
                hash,
                header,
                body: v2,
            }),
        };

        block.verify()?;
        Ok(block)
    }

    /// Returns the reference to the header.    
    pub fn header(&self) -> &BlockHeader {
        match self {
            VersionedBlock::V1(v1) => v1.header(),
            VersionedBlock::V2(v2) => v2.header(),
        }
    }

    /// Returns the block's header, consuming `self`.
    pub fn take_header(self) -> BlockHeader {
        match self {
            VersionedBlock::V1(v1) => v1.take_header(),
            VersionedBlock::V2(v2) => v2.take_header(),
        }
    }

    /// Returns the timestamp from when the block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            VersionedBlock::V1(v1) => v1.header.timestamp(),
            VersionedBlock::V2(v2) => v2.header.timestamp(),
        }
    }

    /// Returns the protocol version of the network from when this block was created.
    pub fn protocol_version(&self) -> ProtocolVersion {
        match self {
            VersionedBlock::V1(v1) => v1.header.protocol_version(),
            VersionedBlock::V2(v2) => v2.header.protocol_version(),
        }
    }

    /// The hash of this block's header.
    pub fn hash(&self) -> &BlockHash {
        match self {
            VersionedBlock::V1(v1) => v1.hash(),
            VersionedBlock::V2(v2) => v2.hash(),
        }
    }

    /// The block body.
    pub fn body(&self) -> VersionedBlockBody {
        match self {
            VersionedBlock::V1(v1) => VersionedBlockBody::V1(v1.body().clone()),
            VersionedBlock::V2(v2) => VersionedBlockBody::V2(v2.body().clone()),
        }
    }

    /// Check the integrity of a block by hashing its body and header
    pub fn verify(&self) -> Result<(), BlockValidationError> {
        match self {
            VersionedBlock::V1(v1) => v1.verify(),
            VersionedBlock::V2(v2) => v2.verify(),
        }
    }

    /// Returns the height of this block, i.e. the number of ancestors.
    pub fn height(&self) -> u64 {
        match self {
            VersionedBlock::V1(v1) => v1.header.height(),
            VersionedBlock::V2(v2) => v2.header.height(),
        }
    }

    /// Returns the deploy hashes within the block.
    pub fn deploy_hashes(&self) -> &[DeployHash] {
        match self {
            VersionedBlock::V1(v1) => v1.body.deploy_hashes(),
            VersionedBlock::V2(v2) => v2.body.deploy_hashes(),
        }
    }

    /// Returns the era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        match self {
            VersionedBlock::V1(v1) => v1.era_id(),
            VersionedBlock::V2(v2) => v2.era_id(),
        }
    }

    /// Returns `true` if this block is the last one in the current era.
    pub fn is_switch_block(&self) -> bool {
        match self {
            VersionedBlock::V1(v1) => v1.header.is_switch_block(),
            VersionedBlock::V2(v2) => v2.header.is_switch_block(),
        }
    }

    /// Returns the transfer hashes within the block.
    pub fn transfer_hashes(&self) -> &[DeployHash] {
        match self {
            VersionedBlock::V1(v1) => v1.body.transfer_hashes(),
            VersionedBlock::V2(v2) => v2.body.transfer_hashes(),
        }
    }

    /// Returns the deploy and transfer hashes in the order in which they were executed.
    pub fn deploy_and_transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> {
        self.deploy_hashes()
            .iter()
            .chain(self.transfer_hashes().iter())
    }

    /// Returns the root hash of global state after the deploys in this block have been executed.
    pub fn state_root_hash(&self) -> &Digest {
        match self {
            VersionedBlock::V1(v1) => v1.header.state_root_hash(),
            VersionedBlock::V2(v2) => v2.header.state_root_hash(),
        }
    }
}

impl Display for VersionedBlock {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            VersionedBlock::V1(v1) => fmt::Display::fmt(&v1, f),
            VersionedBlock::V2(v2) => fmt::Display::fmt(&v2, f),
        }
    }
}

impl ToBytes for VersionedBlock {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            VersionedBlock::V1(v1) => {
                buffer.insert(0, BLOCK_V1_TAG);
                buffer.extend(v1.to_bytes()?);
            }
            VersionedBlock::V2(v2) => {
                buffer.insert(0, BLOCK_V2_TAG);
                buffer.extend(v2.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                VersionedBlock::V1(v1) => v1.serialized_length(),
                VersionedBlock::V2(v2) => v2.serialized_length(),
            }
    }
}

impl FromBytes for VersionedBlock {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            BLOCK_V1_TAG => {
                let (body, remainder): (BlockV1, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V1(body), remainder))
            }
            BLOCK_V2_TAG => {
                let (body, remainder): (BlockV2, _) = FromBytes::from_bytes(remainder)?;
                Ok((Self::V2(body), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl From<&Block> for VersionedBlock {
    fn from(block: &Block) -> Self {
        VersionedBlock::V2(block.clone())
    }
}

impl From<Block> for VersionedBlock {
    fn from(block: Block) -> Self {
        VersionedBlock::V2(block)
    }
}

impl From<&BlockV1> for VersionedBlock {
    fn from(block: &BlockV1) -> Self {
        VersionedBlock::V1(block.clone())
    }
}

impl From<BlockV1> for VersionedBlock {
    fn from(block: BlockV1) -> Self {
        VersionedBlock::V1(block)
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, testing::TestRng};

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block_v1 = BlockV1::random(rng);
        let versioned_block = VersionedBlock::V1(block_v1);
        bytesrepr::test_serialization_roundtrip(&versioned_block);

        let block_v2 = BlockV2::random(rng);
        let versioned_block = VersionedBlock::V2(block_v2);
        bytesrepr::test_serialization_roundtrip(&versioned_block);
    }
}
