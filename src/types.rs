//! Common types used across multiple components.

mod block;
mod deploy;

pub use block::{Block, BlockHash, BlockHeader};
pub use deploy::{Deploy, DeployHash, DeployHeader};
