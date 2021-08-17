//! Errors that may be emitted by methods for common types.

use std::collections::BTreeMap;

use casper_types::{bytesrepr, PublicKey, U512};
use thiserror::Error;

use crate::{
    crypto::hash::Digest,
    types::{block::EraReport, Block, BlockHash},
};

/// An error that can arise when creating a block from a finalized block and other components
#[derive(Error, Debug)]
pub enum BlockCreationError {
    /// `EraEnd`s need both an `EraReport` present and a map of the next era validator weights.
    /// If one of them is not present while trying to construct an `EraEnd` we must emit an
    /// error.
    #[error(
        "Cannot create EraEnd unless we have both an EraReport and next era validators. \
         Era report: {maybe_era_report:?}, \
         Next era validator weights: {maybe_next_era_validator_weights:?}"
    )]
    CouldNotCreateEraEnd {
        /// An optional `EraReport` we tried to use to construct an `EraEnd`
        maybe_era_report: Option<EraReport>,
        /// An optional map of the next era validator weights used to construct an `EraEnd`
        maybe_next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
    },

    /// Wrapper of [`blake2::digest::InvalidOutputSize`]; occurs when trying to construct
    /// an `EraEnd`.
    #[error(transparent)]
    Blake2bDigestInvalidOutputSize(#[from] blake2::digest::InvalidOutputSize),
}

/// An error that can arise when validating a block's cryptographic integrity using its hashes
#[derive(Error, Debug)]
pub enum BlockValidationError {
    /// Problem serializing some of a block's data into bytes
    #[error(transparent)]
    BytesReprError(#[from] bytesrepr::Error),

    /// The body hash in the header is not the same as the hash of the body of the block
    #[error(
        "Block header has incorrect body hash. \
         Actual block body hash: {actual_block_body_hash:?}, \
         Block: {block:?}"
    )]
    UnexpectedBodyHash {
        /// The `Block` with the `BlockHeader` with the incorrect block body hash
        block: Box<Block>,
        /// The actual hash of the block's `BlockBody`
        actual_block_body_hash: Digest,
    },

    /// The block's hash is not the same as the header's hash
    #[error(
        "Block has incorrect block hash. \
         Actual block body hash: {actual_block_header_hash:?}, \
         Block: {block:?}"
    )]
    UnexpectedBlockHash {
        /// The `Block` with the incorrect `BlockHeaderHash`
        block: Box<Block>,
        /// The actual hash of the block's `BlockHeader`
        actual_block_header_hash: BlockHash,
    },
}
