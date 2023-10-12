//! Common types used across multiple components.

pub(crate) mod appendable_block;
mod available_block_range;
mod block;
mod chunkable;
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
mod transaction;
mod validator_matrix;
mod value_or_chunk;

use rand::{CryptoRng, RngCore};
#[cfg(not(test))]
use rand_chacha::ChaCha20Rng;

pub use available_block_range::AvailableBlockRange;
pub(crate) use block::{
    compute_approvals_checksum, create_single_block_rewarded_signatures, ApprovalsHashes,
    BlockExecutionResultsOrChunkId, BlockPayload, BlockWithMetadata, ForwardMetaBlock, MetaBlock,
    MetaBlockMergeError, MetaBlockState,
};
pub use block::{
    BlockExecutionResultsOrChunk, ExecutableBlock, FinalizedBlock, InternalEraReport, SignedBlock,
};
pub use chunkable::Chunkable;
pub use datasize::DataSize;
pub use exit_code::ExitCode;
pub(crate) use max_ttl::MaxTtl;
pub use node_config::{NodeConfig, SyncHandling};
pub(crate) use node_id::NodeId;
pub use peers_map::PeersMap;
pub use status_feed::{ChainspecInfo, GetStatusResult, StatusFeed};
pub(crate) use sync_leap::{GlobalStatesMetadata, SyncLeap, SyncLeapIdentifier};
pub(crate) use transaction::{
    DeployExecutionInfo, DeployHashWithApprovals, DeployOrTransferHash,
    DeployWithFinalizedApprovals, FinalizedApprovals, FinalizedDeployApprovals,
    FinalizedTransactionV1Approvals, LegacyDeploy, TransactionWithFinalizedApprovals,
};
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
