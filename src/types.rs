//! Common types used across multiple components.

mod block;
mod deploy;

pub use block::{Block, BlockHash, BlockHeader, ExecutedBlock, ProtoBlock};
pub use deploy::{DecodingError, Deploy, DeployHash, DeployHeader, EncodingError};
