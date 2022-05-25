//! Common types used across multiple components.

pub(crate) mod appendable_block;
mod available_block_range;
mod block;
pub mod chainspec;
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

use rand::{CryptoRng, RngCore};
#[cfg(not(test))]
use rand_chacha::ChaCha20Rng;

pub use available_block_range::{AvailableBlockRange, AvailableBlockRangeError};
pub use block::{
    json_compatibility::{JsonBlock, JsonBlockHeader},
    Block, BlockAndDeploys, BlockBody, BlockHash, BlockHeader, BlockSignatures, FinalitySignature,
    FinalizedBlock, HashingAlgorithmVersion, MerkleBlockBody, MerkleBlockBodyPart,
    MerkleLinkedListNode,
};
pub(crate) use block::{
    BlockHashAndHeight, BlockHeaderWithMetadata, BlockPayload, BlockWithMetadata,
};
pub use chainspec::Chainspec;
pub(crate) use chainspec::{ActivationPoint, ChainspecRawBytes};
pub use datasize::DataSize;
pub use deploy::{
    Approval, Deploy, DeployConfigurationFailure, DeployHash, DeployHeader, DeployMetadata,
    DeployMetadataExt, DeployOrTransferHash, DeployWithApprovals, DeployWithFinalizedApprovals,
    Error as DeployError, ExcessiveSizeError as ExcessiveSizeDeployError, FinalizedApprovals,
    FinalizedApprovalsWithId,
};
pub use error::BlockValidationError;
pub use exit_code::ExitCode;
pub(crate) use item::{Item, Tag};
pub use node_config::NodeConfig;
pub(crate) use node_id::NodeId;
pub use peers_map::PeersMap;
pub use status_feed::{ChainspecInfo, GetStatusResult, NodeState, StatusFeed};

/// An object-safe RNG trait that requires a cryptographically strong random number generator.
pub trait CryptoRngCore: CryptoRng + RngCore {}

impl<T> CryptoRngCore for T where T: CryptoRng + RngCore + ?Sized {}

/// The cryptographically secure RNG used throughout the node.
#[cfg(not(test))]
pub type NodeRng = ChaCha20Rng;

/// The RNG used throughout the node for testing.
#[cfg(test)]
pub type NodeRng = casper_types::testing::TestRng;
