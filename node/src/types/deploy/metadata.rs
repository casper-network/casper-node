use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::error;

use casper_types::{ExecutionResult, Transfer};

use crate::types::{BlockHash, BlockHashAndHeight};

/// The deploy mutable metadata.
///
/// Currently a stop-gap measure to associate an immutable deploy with additional metadata. Holds
/// execution results.
#[derive(Clone, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub(crate) struct Metadata {
    /// The block hashes of blocks containing the related deploy, along with the results of
    /// executing the related deploy in the context of one or more blocks.
    pub(crate) execution_results: HashMap<BlockHash, ExecutionResult>,
}

impl Metadata {
    pub(crate) fn successful_transfers(&self, block_hash: &BlockHash) -> Vec<Transfer> {
        match self.execution_results.get(block_hash) {
            Some(exec_result) => exec_result.successful_transfers(),
            None => {
                error!(
                    execution_results = ?self.execution_results,
                    %block_hash,
                    "should have exec result"
                );
                vec![]
            }
        }
    }
}

/// Additional information describing a deploy.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub(crate) enum MetadataExt {
    /// Holds the execution results of a deploy.
    Metadata(Metadata),
    /// Holds the hash and height of the block this deploy was included in.
    BlockInfo(BlockHashAndHeight),
    /// No execution results or block information available.
    Empty,
}

impl Default for MetadataExt {
    fn default() -> Self {
        Self::Empty
    }
}

impl From<Metadata> for MetadataExt {
    fn from(deploy_metadata: Metadata) -> Self {
        Self::Metadata(deploy_metadata)
    }
}

impl From<BlockHashAndHeight> for MetadataExt {
    fn from(deploy_block_info: BlockHashAndHeight) -> Self {
        Self::BlockInfo(deploy_block_info)
    }
}

impl PartialEq<Metadata> for MetadataExt {
    fn eq(&self, other: &Metadata) -> bool {
        match self {
            Self::Metadata(metadata) => *metadata == *other,
            _ => false,
        }
    }
}

impl PartialEq<BlockHashAndHeight> for MetadataExt {
    fn eq(&self, other: &BlockHashAndHeight) -> bool {
        match self {
            Self::BlockInfo(block_hash_and_height) => *block_hash_and_height == *other,
            _ => false,
        }
    }
}
