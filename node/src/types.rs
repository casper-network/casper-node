//! Common types used across multiple components.

pub(crate) mod appendable_block;
mod available_block_range;
mod block;
pub mod chainspec;
mod chunkable;
mod deploy;
pub mod error;
mod exit_code;
mod item;
pub mod json_compatibility;
mod node_config;
mod node_id;
/// Peers map.
pub mod peers_map;
mod status_feed;
mod sync_leap;
mod validator_matrix;
mod value_or_chunk;

use rand::{CryptoRng, RngCore};
#[cfg(not(test))]
use rand_chacha::ChaCha20Rng;

pub use available_block_range::AvailableBlockRange;
pub(crate) use block::{
    compute_approvals_checksum, ApprovalsHashes, BlockHashAndHeight, BlockHeaderWithMetadata,
    BlockPayload, BlockWithMetadata, FinalitySignatureId,
};
pub use block::{
    json_compatibility::{JsonBlock, JsonBlockHeader},
    Block, BlockAndDeploys, BlockBody, BlockExecutionResultsOrChunk,
    BlockExecutionResultsOrChunkId, BlockExecutionResultsOrChunkIdDisplay, BlockHash, BlockHeader,
    BlockSignatures, FinalitySignature, FinalizedBlock,
};
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
pub(crate) use item::{EmptyValidationMetadata, FetcherItem, GossiperItem, Item, Tag};
pub use node_config::NodeConfig;
pub(crate) use node_id::NodeId;
pub use peers_map::PeersMap;
pub use status_feed::{ChainspecInfo, GetStatusResult, StatusFeed};
pub(crate) use sync_leap::{SyncLeap, SyncLeapIdentifier};
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
