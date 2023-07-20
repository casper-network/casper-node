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
mod versioned_block;

use alloc::boxed::Box;
use core::fmt::{self, Display, Formatter};
#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use serde::Serialize;

use self::block_v2::BlockV2;
use crate::{bytesrepr, Digest};
pub use block_body::{BlockBody, BlockBodyV1, BlockBodyV2, VersionedBlockBody};
pub use block_hash::BlockHash;
pub use block_hash_and_height::BlockHashAndHeight;
pub use block_header::BlockHeader;
pub use block_signatures::{BlockSignatures, BlockSignaturesMergeError};
pub use block_v1::BlockV1;
pub use era_end::EraEnd;
pub use era_report::EraReport;
pub use finality_signature::FinalitySignature;
pub use finality_signature_id::FinalitySignatureId;
#[cfg(all(feature = "std", feature = "json-schema"))]
pub use json_compatibility::{
    JsonBlock, JsonBlockBody, JsonBlockHeader, JsonEraEnd, JsonEraReport, JsonProof, JsonReward,
    JsonValidatorWeight,
};
pub use signed_block_header::{SignedBlockHeader, SignedBlockHeaderValidationError};
pub use versioned_block::VersionedBlock;

/// A block.
pub type Block = BlockV2;

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
        block: Box<VersionedBlock>,
        /// The actual hash of the block.
        actual_block_hash: BlockHash,
    },
    /// The body hash in the header is not the same as the actual hash of the body of the block.
    UnexpectedBodyHash {
        /// The block with the header containing the incorrect block body hash.
        block: Box<VersionedBlock>,
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
