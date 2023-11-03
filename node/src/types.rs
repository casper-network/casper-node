//! Common types used across multiple components.

pub(crate) mod appendable_block;
mod available_block_range;
mod block;
mod block_hash_height_and_era;
pub mod chainspec;
mod chunkable;
mod deploy;
pub mod error;
mod exit_code;
pub mod json_compatibility;
mod max_ttl;
mod node_config;
mod node_id;
/// Peers map.
pub mod peers_map;
mod status_feed;
mod sync_leap;
pub(crate) mod sync_leap_validation_metadata;
mod validator_matrix;
mod value_or_chunk;

use rand::{CryptoRng, RngCore};
#[cfg(not(test))]
use rand_chacha::ChaCha20Rng;

pub use available_block_range::AvailableBlockRange;
pub(crate) use block::{
    compute_approvals_checksum, ApprovalsHashes, BlockHashAndHeight, BlockHeaderWithMetadata,
    BlockPayload, BlockWithMetadata, FinalitySignatureId, MetaBlock, MetaBlockMergeError,
    MetaBlockState,
};
pub use block::{
    json_compatibility::{JsonBlock, JsonBlockHeader},
    Block, BlockAndDeploys, BlockBody, BlockExecutionResultsOrChunk,
    BlockExecutionResultsOrChunkId, BlockExecutionResultsOrChunkIdDisplay, BlockHash, BlockHeader,
    BlockSignatures, FinalitySignature, FinalizedBlock,
};
pub(crate) use block_hash_height_and_era::BlockHashHeightAndEra;
pub use chainspec::Chainspec;
pub(crate) use chainspec::{ActivationPoint, ChainspecRawBytes};
pub use chunkable::Chunkable;
pub use datasize::DataSize;
pub use deploy::{
    Approval, ApprovalsHash, Deploy, DeployConfigurationFailure, DeployError, DeployHash,
    DeployHeader, DeployOrTransferHash, ExcessiveSizeError as ExcessiveSizeDeployError,
};
pub(crate) use deploy::{
    DeployFootprint, DeployHashWithApprovals, DeployId, DeployMetadata, DeployMetadataExt,
    DeployWithFinalizedApprovals, FinalizedApprovals, LegacyDeploy,
};
pub use error::BlockValidationError;
pub use exit_code::ExitCode;
pub(crate) use max_ttl::MaxTtl;
pub use node_config::{NodeConfig, SyncHandling};
pub(crate) use node_id::NodeId;
pub use peers_map::PeersMap;
pub use status_feed::{ChainspecInfo, GetStatusResult, StatusFeed};
pub(crate) use sync_leap::{GlobalStatesMetadata, SyncLeap, SyncLeapIdentifier};
pub(crate) use validator_matrix::{EraValidatorWeights, SignatureWeight, ValidatorMatrix};
pub use value_or_chunk::{
    ChunkingError, TrieOrChunk, TrieOrChunkId, TrieOrChunkIdDisplay, ValueOrChunk,
};

/// An object-safe RNG trait that requires a cryptographically strong random number generator.
pub trait CryptoRngCore: CryptoRng + RngCore {}

impl<T> CryptoRngCore for T where T: CryptoRng + RngCore + ?Sized {}

/// The cryptographically secure RNG used throughout the node.
#[cfg(not(test))]
pub type NodeRng = ChaCha20Rng;

/// The RNG used throughout the node for testing.
#[cfg(test)]
pub type NodeRng = casper_types::testing::TestRng;

#[cfg(test)]
pub(crate) use block::test_block_builder::TestBlockBuilder;
