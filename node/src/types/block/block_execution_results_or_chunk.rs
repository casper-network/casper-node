use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::{self, ToBytes},
    execution::ExecutionResult,
    BlockHash, ChunkWithProofVerificationError, Digest,
};
#[cfg(test)]
use casper_types::{execution::ExecutionJournal, U512};

use super::BlockExecutionResultsOrChunkId;
use crate::{
    components::{
        block_synchronizer::ExecutionResultsChecksum,
        fetcher::{FetchItem, Tag},
    },
    types::{Chunkable, ValueOrChunk},
    utils::ds,
};

/// Represents execution results for all deploys in a single block or a chunk of this complete
/// value.
#[derive(Clone, Serialize, Deserialize, Debug, Eq, DataSize)]
pub struct BlockExecutionResultsOrChunk {
    /// Block to which this value or chunk refers to.
    pub(super) block_hash: BlockHash,
    /// Complete execution results for the block or a chunk of the complete data.
    pub(super) value: ValueOrChunk<Vec<ExecutionResult>>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    pub(super) is_valid: OnceCell<Result<bool, bytesrepr::Error>>,
}

impl BlockExecutionResultsOrChunk {
    /// Verifies equivalence of the effects (or chunks) Merkle root hash with the expected value.
    pub fn validate(&self, expected_merkle_root: &Digest) -> Result<bool, bytesrepr::Error> {
        *self.is_valid.get_or_init(|| match &self.value {
            ValueOrChunk::Value(block_execution_results) => {
                Ok(&Chunkable::hash(&block_execution_results)? == expected_merkle_root)
            }
            ValueOrChunk::ChunkWithProof(chunk_with_proof) => {
                Ok(&chunk_with_proof.proof().root_hash() == expected_merkle_root)
            }
        })
    }

    /// Consumes `self` and returns inner `ValueOrChunk` field.
    pub fn into_value(self) -> ValueOrChunk<Vec<ExecutionResult>> {
        self.value
    }

    /// Returns the hash of the block this execution result belongs to.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    #[cfg(test)]
    pub(crate) fn new_mock_value(block_hash: BlockHash) -> Self {
        Self {
            block_hash,
            value: ValueOrChunk::Value(vec![ExecutionResult::Success {
                effects: ExecutionJournal::new(),
                transfers: vec![],
                cost: U512::from(123),
            }]),
            is_valid: OnceCell::with_value(Ok(true)),
        }
    }
}

impl PartialEq for BlockExecutionResultsOrChunk {
    fn eq(&self, other: &BlockExecutionResultsOrChunk) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let BlockExecutionResultsOrChunk {
            block_hash,
            value,
            is_valid: _,
        } = self;
        *block_hash == other.block_hash && *value == other.value
    }
}

impl FetchItem for BlockExecutionResultsOrChunk {
    type Id = BlockExecutionResultsOrChunkId;
    type ValidationError = ChunkWithProofVerificationError;
    type ValidationMetadata = ExecutionResultsChecksum;

    const TAG: Tag = Tag::BlockExecutionResults;

    fn fetch_id(&self) -> Self::Id {
        let chunk_index = match &self.value {
            ValueOrChunk::Value(_) => 0,
            ValueOrChunk::ChunkWithProof(chunks) => chunks.proof().index(),
        };
        BlockExecutionResultsOrChunkId {
            chunk_index,
            block_hash: self.block_hash,
        }
    }

    fn validate(&self, metadata: &ExecutionResultsChecksum) -> Result<(), Self::ValidationError> {
        if let ValueOrChunk::ChunkWithProof(chunk_with_proof) = &self.value {
            chunk_with_proof.verify()?;
        }
        if let ExecutionResultsChecksum::Checkable(expected) = *metadata {
            if !self
                .validate(&expected)
                .map_err(ChunkWithProofVerificationError::Bytesrepr)?
            {
                return Err(ChunkWithProofVerificationError::UnexpectedRootHash);
            }
        }
        Ok(())
    }
}

impl Display for BlockExecutionResultsOrChunk {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let size = match &self.value {
            ValueOrChunk::Value(exec_results) => exec_results.serialized_length(),
            ValueOrChunk::ChunkWithProof(chunk) => chunk.serialized_length(),
        };
        write!(
            f,
            "block execution results or chunk ({size} bytes) for block {}",
            self.block_hash.inner()
        )
    }
}
mod specimen_support {
    use crate::utils::specimen::{Cache, LargestSpecimen, SizeEstimator};

    use super::BlockExecutionResultsOrChunk;
    use once_cell::sync::OnceCell;

    impl LargestSpecimen for BlockExecutionResultsOrChunk {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            BlockExecutionResultsOrChunk {
                block_hash: LargestSpecimen::largest_specimen(estimator, cache),
                value: LargestSpecimen::largest_specimen(estimator, cache),
                is_valid: OnceCell::with_value(Ok(true)),
            }
        }
    }
}
