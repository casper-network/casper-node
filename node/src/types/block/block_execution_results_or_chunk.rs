use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use casper_types::{
    bytesrepr::{self, ToBytes},
    execution::VersionedExecutionResult,
    BlockHash, ChunkWithProof, ChunkWithProofVerificationError, Digest,
};
#[cfg(test)]
use casper_types::{
    execution::{ExecutionJournal, ExecutionResultV2},
    U512,
};

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
    pub(super) value: ValueOrChunk<Vec<VersionedExecutionResult>>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    pub(super) is_valid: OnceCell<Result<bool, bytesrepr::Error>>,
}

impl BlockExecutionResultsOrChunk {
    pub(crate) fn new(
        block_hash: BlockHash,
        chunk_index: u64,
        execution_results: Vec<VersionedExecutionResult>,
    ) -> Option<Self> {
        fn make_value_or_chunk<T: Chunkable>(
            data: T,
            block_hash: &BlockHash,
            chunk_index: u64,
        ) -> Option<ValueOrChunk<T>> {
            match ValueOrChunk::new(data, chunk_index) {
                Ok(value_or_chunk) => Some(value_or_chunk),
                Err(error) => {
                    error!(
                        %block_hash, %chunk_index, %error,
                        "failed to construct `BlockExecutionResultsOrChunk`"
                    );
                    None
                }
            }
        }

        let is_v1 = matches!(
            execution_results.first(),
            Some(VersionedExecutionResult::V1(_))
        );

        // If it's not V1, just construct the `ValueOrChunk` from `Vec<VersionedExecutionResult>`.
        if !is_v1 {
            let value = make_value_or_chunk(execution_results, &block_hash, chunk_index)?;
            return Some(BlockExecutionResultsOrChunk {
                block_hash,
                value,
                is_valid: OnceCell::new(),
            });
        }

        // If it is V1, we need to construct the `ValueOrChunk` from a `Vec<ExecutionResultV1>` if
        // it's big enough to need chunked, otherwise we need to use the
        // `Vec<VersionedExecutionResult>` as the `ValueOrChunk::Value`.
        let mut v1_results = Vec::with_capacity(execution_results.len());
        for result in &execution_results {
            if let VersionedExecutionResult::V1(v1_result) = result {
                v1_results.push(v1_result);
            } else {
                error!(
                    ?execution_results,
                    "all execution results should be version 1"
                );
                return None;
            }
        }
        if v1_results.serialized_length() <= ChunkWithProof::CHUNK_SIZE_BYTES {
            let value = make_value_or_chunk(execution_results, &block_hash, chunk_index)?;
            return Some(BlockExecutionResultsOrChunk {
                block_hash,
                value,
                is_valid: OnceCell::new(),
            });
        }

        let v1_value = make_value_or_chunk(v1_results, &block_hash, chunk_index)?;
        let value = match v1_value {
            ValueOrChunk::Value(_) => {
                error!(
                    ?execution_results,
                    "v1 execution results of this size should be chunked"
                );
                return None;
            }
            ValueOrChunk::ChunkWithProof(chunk) => ValueOrChunk::ChunkWithProof(chunk),
        };

        Some(BlockExecutionResultsOrChunk {
            block_hash,
            value,
            is_valid: OnceCell::new(),
        })
    }

    /// Verifies equivalence of the execution results (or chunks) Merkle root hash with the
    /// expected value.
    pub fn validate(&self, expected: &Digest) -> Result<bool, bytesrepr::Error> {
        *self.is_valid.get_or_init(|| match &self.value {
            ValueOrChunk::Value(block_execution_results) => {
                // If results is not empty and all are V1, convert and verify.
                let is_v1 = matches!(
                    block_execution_results.first(),
                    Some(VersionedExecutionResult::V1(_))
                );
                let actual = if is_v1 {
                    let mut v1_results = Vec::with_capacity(block_execution_results.len());
                    for result in block_execution_results {
                        if let VersionedExecutionResult::V1(v1_result) = result {
                            v1_results.push(v1_result);
                        } else {
                            debug!(
                                ?block_execution_results,
                                "all execution results should be version 1"
                            );
                            return Ok(false);
                        }
                    }
                    Chunkable::hash(&v1_results)?
                } else {
                    Chunkable::hash(&block_execution_results)?
                };
                Ok(&actual == expected)
            }
            ValueOrChunk::ChunkWithProof(chunk_with_proof) => {
                Ok(&chunk_with_proof.proof().root_hash() == expected)
            }
        })
    }

    /// Consumes `self` and returns inner `ValueOrChunk` field.
    pub fn into_value(self) -> ValueOrChunk<Vec<VersionedExecutionResult>> {
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
            value: ValueOrChunk::Value(vec![VersionedExecutionResult::V2(
                ExecutionResultV2::Success {
                    effects: ExecutionJournal::new(),
                    transfers: vec![],
                    cost: U512::from(123),
                },
            )]),
            is_valid: OnceCell::with_value(Ok(true)),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_from_value(
        block_hash: BlockHash,
        value: ValueOrChunk<Vec<VersionedExecutionResult>>,
    ) -> Self {
        Self {
            block_hash,
            value,
            is_valid: OnceCell::new(),
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

#[cfg(test)]
mod tests {
    use rand::Rng;

    use casper_types::{execution::ExecutionResultV1, testing::TestRng, ChunkWithProof};

    use super::*;
    use crate::contract_runtime::compute_execution_results_checksum;

    fn compute_execution_results_v1_checksum(
        v1_execution_results: Vec<&ExecutionResultV1>,
    ) -> ExecutionResultsChecksum {
        ExecutionResultsChecksum::Checkable(v1_execution_results.hash().unwrap())
    }

    #[test]
    fn should_validate_v1_unchunked_checksum() {
        let rng = &mut TestRng::new();
        let execution_results = vec![
            VersionedExecutionResult::V1(rng.gen()),
            VersionedExecutionResult::V1(rng.gen()),
        ];
        let checksum = compute_execution_results_v1_checksum(
            execution_results
                .iter()
                .map(|exec_result| match exec_result {
                    VersionedExecutionResult::V1(exec_result) => exec_result,
                    _ => unreachable!(),
                })
                .collect(),
        );

        let block_hash = BlockHash::random(rng);
        let block_results =
            BlockExecutionResultsOrChunk::new(block_hash, 0, execution_results).unwrap();
        // Ensure the results weren't chunked.
        assert!(matches!(block_results.value, ValueOrChunk::Value(_)));

        FetchItem::validate(&block_results, &checksum).unwrap();
    }

    #[test]
    fn should_validate_v1_chunked_checksum() {
        let rng = &mut TestRng::new();

        let v1_result: ExecutionResultV1 = rng.gen();
        // Ensure we fill with enough copies to cause three chunks.
        let count = (2 * ChunkWithProof::CHUNK_SIZE_BYTES / v1_result.serialized_length()) + 1;
        let execution_results = vec![VersionedExecutionResult::V1(v1_result); count];
        let checksum = compute_execution_results_v1_checksum(
            execution_results
                .iter()
                .map(|exec_result| match exec_result {
                    VersionedExecutionResult::V1(exec_result) => exec_result,
                    _ => unreachable!(),
                })
                .collect(),
        );

        let block_hash = BlockHash::random(rng);
        for chunk_index in 0..3 {
            let block_results = BlockExecutionResultsOrChunk::new(
                block_hash,
                chunk_index,
                execution_results.clone(),
            )
            .unwrap();
            // Ensure the results were chunked.
            assert!(matches!(
                block_results.value,
                ValueOrChunk::ChunkWithProof(_)
            ));

            FetchItem::validate(&block_results, &checksum).unwrap();
        }
    }

    #[test]
    fn should_validate_v1_empty_checksum() {
        let rng = &mut TestRng::new();
        let checksum = compute_execution_results_v1_checksum(vec![]);

        let block_results =
            BlockExecutionResultsOrChunk::new(BlockHash::random(rng), 0, vec![]).unwrap();
        FetchItem::validate(&block_results, &checksum).unwrap();
    }

    #[test]
    fn should_validate_versioned_unchunked_checksum() {
        let rng = &mut TestRng::new();
        let execution_results = vec![
            VersionedExecutionResult::from(ExecutionResultV2::random(rng)),
            VersionedExecutionResult::from(ExecutionResultV2::random(rng)),
        ];
        let checksum = ExecutionResultsChecksum::Checkable(
            compute_execution_results_checksum(execution_results.iter()).unwrap(),
        );

        let block_hash = BlockHash::random(rng);
        let block_results =
            BlockExecutionResultsOrChunk::new(block_hash, 0, execution_results).unwrap();
        // Ensure the results weren't chunked.
        assert!(matches!(block_results.value, ValueOrChunk::Value(_)));

        FetchItem::validate(&block_results, &checksum).unwrap();
    }

    #[test]
    fn should_validate_versioned_chunked_checksum() {
        let rng = &mut TestRng::new();

        let v2_result = ExecutionResultV2::random(rng);
        // Ensure we fill with enough copies to cause three chunks.
        let count = (2 * ChunkWithProof::CHUNK_SIZE_BYTES / v2_result.serialized_length()) + 1;
        let execution_results = vec![VersionedExecutionResult::V2(v2_result); count];
        let checksum = ExecutionResultsChecksum::Checkable(
            compute_execution_results_checksum(execution_results.iter()).unwrap(),
        );

        let block_hash = BlockHash::random(rng);
        for chunk_index in 0..3 {
            let block_results = BlockExecutionResultsOrChunk::new(
                block_hash,
                chunk_index,
                execution_results.clone(),
            )
            .unwrap();
            // Ensure the results were chunked.
            assert!(matches!(
                block_results.value,
                ValueOrChunk::ChunkWithProof(_)
            ));

            FetchItem::validate(&block_results, &checksum).unwrap();
        }
    }

    #[test]
    fn should_validate_versioned_empty_checksum() {
        let rng = &mut TestRng::new();
        let checksum = ExecutionResultsChecksum::Checkable(
            compute_execution_results_checksum(None.into_iter()).unwrap(),
        );

        let block_results =
            BlockExecutionResultsOrChunk::new(BlockHash::random(rng), 0, vec![]).unwrap();
        FetchItem::validate(&block_results, &checksum).unwrap();
    }
}
