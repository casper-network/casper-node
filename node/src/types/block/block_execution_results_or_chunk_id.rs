use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::BlockHash;

/// ID of the request for block execution results or chunk.
#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub(crate) struct BlockExecutionResultsOrChunkId {
    /// Index of the chunk being requested.
    pub(super) chunk_index: u64,
    /// Hash of the block.
    pub(super) block_hash: BlockHash,
}

impl BlockExecutionResultsOrChunkId {
    /// Returns an instance of post-1.5 request for block execution results.
    /// The `chunk_index` is set to 0 as the starting point of the fetch cycle.
    /// If the effects are stored without chunking the index will be 0 as well.
    pub fn new(block_hash: BlockHash) -> Self {
        BlockExecutionResultsOrChunkId {
            chunk_index: 0,
            block_hash,
        }
    }

    /// Returns the request for the `next_chunk` retaining the original request's block hash.
    pub fn next_chunk(&self, next_chunk: u64) -> Self {
        BlockExecutionResultsOrChunkId {
            chunk_index: next_chunk,
            block_hash: self.block_hash,
        }
    }

    pub(crate) fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    pub(crate) fn chunk_index(&self) -> u64 {
        self.chunk_index
    }
}

impl Display for BlockExecutionResultsOrChunkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "execution results for {} or chunk #{}",
            self.block_hash, self.chunk_index
        )
    }
}

mod specimen_support {
    use crate::utils::specimen::{Cache, LargestSpecimen, SizeEstimator};

    use super::BlockExecutionResultsOrChunkId;

    impl LargestSpecimen for BlockExecutionResultsOrChunkId {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            BlockExecutionResultsOrChunkId {
                chunk_index: u64::MAX,
                block_hash: LargestSpecimen::largest_specimen(estimator, cache),
            }
        }
    }
}
