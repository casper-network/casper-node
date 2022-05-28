//! Errors that may be emitted by methods for common types.

use std::collections::BTreeMap;

use serde::Serialize;
use thiserror::Error;

use casper_hashing::Digest;
use casper_types::{bytesrepr, EraId, PublicKey, U512};

use crate::types::{
    block::EraReport, Block, BlockHash, Deploy, DeployConfigurationFailure, DeployHash,
};

/// An error that can arise when creating a block from a finalized block and other components.
#[derive(Error, Debug, Serialize)]
pub enum BlockCreationError {
    /// `EraEnd`s need both an `EraReport` present and a map of the next era validator weights.
    /// If one of them is not present while trying to construct an `EraEnd` we must emit an
    /// error.
    #[error(
        "cannot create era end unless we have both an era report and next era validators. \
         era report: {maybe_era_report:?}, \
         next era validator weights: {maybe_next_era_validator_weights:?}"
    )]
    CouldNotCreateEraEnd {
        /// An optional `EraReport` we tried to use to construct an `EraEnd`.
        maybe_era_report: Option<EraReport>,
        /// An optional map of the next era validator weights used to construct an `EraEnd`.
        maybe_next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
    },
}

/// An error that can arise when validating a block's cryptographic integrity using its hashes.
#[derive(Error, Debug, Serialize)]
pub enum BlockValidationError {
    /// Problem serializing some of a block's data into bytes.
    #[error("{0}")]
    BytesReprError(bytesrepr::Error),

    /// The body hash in the header is not the same as the hash of the body of the block.
    #[error(
        "block header has incorrect body hash. \
         actual block body hash: {actual_block_body_hash:?}, \
         block: {block:?}"
    )]
    UnexpectedBodyHash {
        /// The `Block` with the `BlockHeader` with the incorrect block body hash.
        block: Box<Block>,
        /// The actual hash of the block's `BlockBody`.
        actual_block_body_hash: Digest,
    },

    /// The block's hash is not the same as the header's hash.
    #[error(
        "block has incorrect block hash. \
         actual block body hash: {actual_block_header_hash:?}, \
         block: {block:?}"
    )]
    UnexpectedBlockHash {
        /// The `Block` with the incorrect `BlockHeaderHash`.
        block: Box<Block>,
        /// The actual hash of the block's `BlockHeader`.
        actual_block_header_hash: BlockHash,
    },

    /// A deploy's hash does not match the hash listed in the `BlockAndDeploys` body.
    #[error(
        "deploy in block-and-deploys has incorrect hash. \
         block: {block:?} \
         invalid_deploy: {invalid_deploy:?} \
         error: {deploy_configuration_failure:?}"
    )]
    UnexpectedDeployHash {
        /// The `Block`.
        block: Box<Block>,
        /// The `Deploy` with the incorrect hash.
        invalid_deploy: Box<Deploy>,
        /// The error.
        deploy_configuration_failure: DeployConfigurationFailure,
    },

    /// A deploy is missing from a `BlockAndDeploys`.
    #[error(
        "deploy in block-and-deploys is missing. \
         block: {block:?} \
         missing_deploy: {missing_deploy}"
    )]
    MissingDeploy {
        /// The `Block` with the missing `Deploy`.
        block: Box<Block>,
        /// The `DeployHash` of the missing `Deploy`.
        missing_deploy: DeployHash,
    },

    /// At least one extra deploy is present in a `BlockAndDeploys`.
    #[error(
        "extra deploy in block-and-deploys. \
         block: {block:?} \
         extra_deploys_count: {extra_deploys_count}"
    )]
    ExtraDeploys {
        /// The `Block` of the `BlockAndDeploys` with the extra `Deploy`.
        block: Box<Block>,
        /// The number of extra deploys provided.
        extra_deploys_count: u32,
    },
}

impl From<bytesrepr::Error> for BlockValidationError {
    fn from(error: bytesrepr::Error) -> Self {
        BlockValidationError::BytesReprError(error)
    }
}

#[derive(Error, Debug)]
pub(crate) enum BlockHeaderWithMetadataValidationError {
    #[error(
        "Finality signatures have unexpected block hash. \
         Expected block hash: {expected_block_hash}, \
         Finality signature block hash: {finality_signatures_block_hash}"
    )]
    FinalitySignaturesHaveUnexpectedBlockHash {
        expected_block_hash: BlockHash,
        finality_signatures_block_hash: BlockHash,
    },
    #[error(
        "Finality signatures have unexpected era id. \
         Expected block hash: {expected_era_id}, \
         Finality signature block hash: {finality_signatures_era_id}"
    )]
    FinalitySignaturesHaveUnexpectedEraId {
        expected_era_id: EraId,
        finality_signatures_era_id: EraId,
    },
}

#[derive(Error, Debug)]
pub(crate) enum BlockWithMetadataValidationError {
    #[error(transparent)]
    BlockValidationError(#[from] BlockValidationError),
    #[error(transparent)]
    BlockHeaderWithMetadataValidationError(#[from] BlockHeaderWithMetadataValidationError),
}
