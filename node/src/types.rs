//! Common types used across multiple components.

mod block;
mod deploy;
mod item;
pub mod json_compatibility;
mod node_config;
mod status_feed;
mod timestamp;

use rand::{CryptoRng, RngCore};

pub use block::{Block, BlockHash, BlockHeader};
pub(crate) use block::{BlockByHeight, BlockLike, FinalizedBlock, ProtoBlock, ProtoBlockHash};
pub use deploy::{Approval, Deploy, DeployHash, DeployHeader, Error as DeployError};
pub use item::{Item, Tag};
pub use node_config::NodeConfig;
pub use status_feed::StatusFeed;
pub use timestamp::{TimeDiff, Timestamp};

/// An object-safe RNG trait that requires a cryptographically strong random number generator.
pub trait CryptoRngCore: CryptoRng + RngCore {}

impl<T> CryptoRngCore for T where T: CryptoRng + RngCore + ?Sized {}
