mod block_body;
mod block_hash;
mod block_hash_and_height;
mod block_header;
mod block_signatures;
mod block_v1;
mod block_v2;
mod era_end;
mod era_report;
mod finality_signature;
mod finality_signature_id;
mod json_compatibility;
mod signed_block_header;
#[cfg(any(feature = "testing", test))]
pub mod test_block_builder;

use alloc::{boxed::Box, vec::Vec};
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;

#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
#[cfg(any(feature = "std", test))]
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    DeployHash, Digest, EraId, ProtocolVersion, Timestamp,
};

pub use block_body::{BlockBody, BlockBodyV1, BlockBodyV2};
pub use block_hash::BlockHash;
pub use block_hash_and_height::BlockHashAndHeight;
pub use block_header::BlockHeader;
pub use block_signatures::{BlockSignatures, BlockSignaturesMergeError};
pub use block_v1::BlockV1;
pub use block_v2::BlockV2;
pub use era_end::EraEnd;
pub use era_report::EraReport;
pub use finality_signature::FinalitySignature;
pub use finality_signature_id::FinalitySignatureId;
#[cfg(all(feature = "std", feature = "json-schema"))]
pub use json_compatibility::JsonBlockWithSignatures;
pub use signed_block_header::{SignedBlockHeader, SignedBlockHeaderValidationError};
#[cfg(any(feature = "testing", test))]
pub use test_block_builder::{FromTestBlockBuilder, TestBlockBuilder};

#[cfg(feature = "json-schema")]
static BLOCK: Lazy<Block> = Lazy::new(|| BlockV2::example().into());

/// An error that can arise when validating a block's cryptographic integrity using its hashes.
#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(any(feature = "std", test), derive(Serialize))]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum BlockValidationError {
    /// Problem serializing some of a block's data into bytes.
    Bytesrepr(bytesrepr::Error),
    /// The provided block's hash is not the same as the actual hash of the block.
    UnexpectedBlockHash {
        /// The block with the incorrect block hash.
        block: Box<Block>,
        /// The actual hash of the block.
        actual_block_hash: BlockHash,
    },
    /// The body hash in the header is not the same as the actual hash of the body of the block.
    UnexpectedBodyHash {
        /// The block with the header containing the incorrect block body hash.
        block: Box<Block>,
        /// The actual hash of the block's body.
        actual_block_body_hash: Digest,
    },
}

impl Display for BlockValidationError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            BlockValidationError::Bytesrepr(error) => {
                write!(formatter, "error validating block: {}", error)
            }
            BlockValidationError::UnexpectedBlockHash {
                block,
                actual_block_hash,
            } => {
                write!(
                    formatter,
                    "block has incorrect block hash - actual block hash: {:?}, block: {:?}",
                    actual_block_hash, block
                )
            }
            BlockValidationError::UnexpectedBodyHash {
                block,
                actual_block_body_hash,
            } => {
                write!(
                    formatter,
                    "block header has incorrect body hash - actual body hash: {:?}, block: {:?}",
                    actual_block_body_hash, block
                )
            }
        }
    }
}

impl From<bytesrepr::Error> for BlockValidationError {
    fn from(error: bytesrepr::Error) -> Self {
        BlockValidationError::Bytesrepr(error)
    }
}

#[cfg(feature = "std")]
impl StdError for BlockValidationError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            BlockValidationError::Bytesrepr(error) => Some(error),
            BlockValidationError::UnexpectedBlockHash { .. }
            | BlockValidationError::UnexpectedBodyHash { .. } => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockConversionError {
    DifferentVersion { expected_version: u8 },
}

#[cfg(feature = "std")]
impl Display for BlockConversionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockConversionError::DifferentVersion { expected_version } => {
                write!(
                    f,
                    "Could not convert a block to the expected version {}",
                    expected_version
                )
            }
        }
    }
}

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for block body v1.
const BLOCK_V1_TAG: u8 = 0;
/// Tag for block body v2.
const BLOCK_V2_TAG: u8 = 1;

/// A block after execution.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    any(feature = "std", feature = "json-schema", test),
    derive(Serialize, Deserialize)
)]
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum Block {
    /// The legacy, initial version of the block.
    #[cfg_attr(
        any(feature = "std", feature = "json-schema", test),
        serde(rename = "Version1")
    )]
    V1(BlockV1),
    /// The version 2 of the block.
    #[cfg_attr(
        any(feature = "std", feature = "json-schema", test),
        serde(rename = "Version2")
    )]
    V2(BlockV2),
}

impl Block {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn new_from_header_and_body(
        header: BlockHeader,
        block_body: BlockBody,
    ) -> Result<Self, Box<BlockValidationError>> {
        let hash = header.block_hash();
        let block = match block_body {
            BlockBody::V1(v1) => Block::V1(BlockV1 {
                hash,
                header,
                body: v1,
            }),
            BlockBody::V2(v2) => Block::V2(BlockV2 {
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
            Block::V1(v1) => v1.header(),
            Block::V2(v2) => v2.header(),
        }
    }

    /// Returns the block's header, consuming `self`.
    pub fn take_header(self) -> BlockHeader {
        match self {
            Block::V1(v1) => v1.take_header(),
            Block::V2(v2) => v2.take_header(),
        }
    }

    /// Returns the timestamp from when the block was proposed.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            Block::V1(v1) => v1.header.timestamp(),
            Block::V2(v2) => v2.header.timestamp(),
        }
    }

    /// Returns the protocol version of the network from when this block was created.
    pub fn protocol_version(&self) -> ProtocolVersion {
        match self {
            Block::V1(v1) => v1.header.protocol_version(),
            Block::V2(v2) => v2.header.protocol_version(),
        }
    }

    /// The hash of this block's header.
    pub fn hash(&self) -> &BlockHash {
        match self {
            Block::V1(v1) => v1.hash(),
            Block::V2(v2) => v2.hash(),
        }
    }

    /// Returns the hash of the block's body.
    pub fn body_hash(&self) -> &Digest {
        match self {
            Block::V1(v1) => v1.header().body_hash(),
            Block::V2(v2) => v2.header().body_hash(),
        }
    }

    /// Returns a random bit needed for initializing a future era.
    pub fn random_bit(&self) -> bool {
        match self {
            Block::V1(v1) => v1.header().random_bit(),
            Block::V2(v2) => v2.header().random_bit(),
        }
    }

    /// Returns the parent block's hash.
    pub fn parent_hash(&self) -> &BlockHash {
        match self {
            Block::V1(v1) => v1.parent_hash(),
            Block::V2(v2) => v2.parent_hash(),
        }
    }

    /// The block body.
    pub fn body(&self) -> BlockBody {
        match self {
            Block::V1(v1) => BlockBody::V1(v1.body().clone()),
            Block::V2(v2) => BlockBody::V2(v2.body().clone()),
        }
    }

    /// Check the integrity of a block by hashing its body and header
    pub fn verify(&self) -> Result<(), BlockValidationError> {
        match self {
            Block::V1(v1) => v1.verify(),
            Block::V2(v2) => v2.verify(),
        }
    }

    /// Returns the height of this block, i.e. the number of ancestors.
    pub fn height(&self) -> u64 {
        match self {
            Block::V1(v1) => v1.header.height(),
            Block::V2(v2) => v2.header.height(),
        }
    }

    /// Returns the deploy hashes within the block.
    pub fn deploy_hashes(&self) -> &[DeployHash] {
        match self {
            Block::V1(v1) => v1.body.deploy_hashes(),
            Block::V2(v2) => v2.body.deploy_hashes(),
        }
    }

    /// Returns the era ID in which this block was created.
    pub fn era_id(&self) -> EraId {
        match self {
            Block::V1(v1) => v1.era_id(),
            Block::V2(v2) => v2.era_id(),
        }
    }

    /// Returns the `EraEnd` of a block if it is a switch block.
    pub fn era_end(&self) -> Option<&EraEnd> {
        match self {
            Block::V1(v1) => v1.header().era_end(),
            Block::V2(v2) => v2.header().era_end(),
        }
    }

    /// Returns `true` if this block is the last one in the current era.
    pub fn is_switch_block(&self) -> bool {
        match self {
            Block::V1(v1) => v1.header.is_switch_block(),
            Block::V2(v2) => v2.header.is_switch_block(),
        }
    }

    /// Returns the transfer hashes within the block.
    pub fn transfer_hashes(&self) -> &[DeployHash] {
        match self {
            Block::V1(v1) => v1.body.transfer_hashes(),
            Block::V2(v2) => v2.body.transfer_hashes(),
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
            Block::V1(v1) => v1.header.state_root_hash(),
            Block::V2(v2) => v2.header.state_root_hash(),
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    #[cfg(feature = "json-schema")]
    pub fn example() -> &'static Self {
        &BLOCK
    }
}

impl Display for Block {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "executed block #{}, {}, timestamp {}, {}, parent {}, post-state hash {}, body hash \
            {}, random bit {}, protocol version: {}",
            self.height(),
            self.hash(),
            self.timestamp(),
            self.era_id(),
            self.parent_hash().inner(),
            self.state_root_hash(),
            self.body_hash(),
            self.random_bit(),
            self.protocol_version()
        )?;
        if let Some(era_end) = self.era_end() {
            write!(formatter, ", era_end: {}", era_end)?;
        }
        Ok(())
    }
}

impl ToBytes for Block {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            Block::V1(v1) => {
                buffer.insert(0, BLOCK_V1_TAG);
                buffer.extend(v1.to_bytes()?);
            }
            Block::V2(v2) => {
                buffer.insert(0, BLOCK_V2_TAG);
                buffer.extend(v2.to_bytes()?);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                Block::V1(v1) => v1.serialized_length(),
                Block::V2(v2) => v2.serialized_length(),
            }
    }
}

impl FromBytes for Block {
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

impl From<&BlockV2> for Block {
    fn from(block: &BlockV2) -> Self {
        Block::V2(block.clone())
    }
}

impl From<BlockV2> for Block {
    fn from(block: BlockV2) -> Self {
        Block::V2(block)
    }
}

impl From<&BlockV1> for Block {
    fn from(block: &BlockV1) -> Self {
        Block::V1(block.clone())
    }
}

impl From<BlockV1> for Block {
    fn from(block: BlockV1) -> Self {
        Block::V1(block)
    }
}

#[cfg(all(feature = "std", feature = "json-schema"))]
impl From<JsonBlockWithSignatures> for Block {
    fn from(block_with_signatures: JsonBlockWithSignatures) -> Self {
        block_with_signatures.block
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, testing::TestRng};

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let block_v1 = BlockV1::build_for_test(TestBlockBuilder::new(), rng);
        let block = Block::V1(block_v1);
        bytesrepr::test_serialization_roundtrip(&block);

        let block_v2 = TestBlockBuilder::new().build(rng);
        let block = Block::V2(block_v2);
        bytesrepr::test_serialization_roundtrip(&block);
    }
}
