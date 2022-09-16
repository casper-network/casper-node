#![allow(unused)] // TODO: To be removed

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
mod shared_object;
mod status_feed;
mod sync_leap;
mod value_or_chunk;

use rand::{CryptoRng, RngCore};
#[cfg(not(test))]
use rand_chacha::ChaCha20Rng;

pub use available_block_range::AvailableBlockRange;
pub use block::{
    json_compatibility::{JsonBlock, JsonBlockHeader},
    Block, BlockAndDeploys, BlockBody, BlockHash, BlockHeader, BlockSignatures, FinalitySignature,
    FinalizedBlock,
};
pub(crate) use block::{
    BlockAdded, BlockAddedValidationError, BlockHashAndHeight, BlockHeaderWithMetadata,
    BlockHeadersBatch, BlockHeadersBatchId, BlockPayload, BlockWithMetadata, FinalitySignatureId,
};
pub use chainspec::Chainspec;
pub(crate) use chainspec::{ActivationPoint, ChainspecRawBytes};
pub use chunkable::Chunkable;
pub use datasize::DataSize;
pub use deploy::{
    Approval, Deploy, DeployConfigurationFailure, DeployFootprint, DeployHash, DeployHeader, DeployMetadata,
    DeployMetadataExt, DeployOrTransferHash, DeployWithApprovals, DeployWithFinalizedApprovals,
    Error as DeployError, ExcessiveSizeError as ExcessiveSizeDeployError, FinalizedApprovals,
    FinalizedApprovalsWithId,
};
pub use error::BlockValidationError;
pub use exit_code::ExitCode;
pub(crate) use item::{FetcherItem, GossiperItem, Item, Tag};
pub use node_config::NodeConfig;
pub(crate) use node_id::NodeId;
pub use peers_map::PeersMap;
pub use status_feed::{ChainspecInfo, GetStatusResult, NodeState, StatusFeed};
pub(crate) use sync_leap::SyncLeap;
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
