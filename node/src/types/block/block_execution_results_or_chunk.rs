use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

#[cfg(test)]
use casper_types::execution::ExecutionResultV2;
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, ToBytes},
    execution::ExecutionResult,
    BlockHash, ChunkWithProof, ChunkWithProofVerificationError, Digest,
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
    pub(super) value: ValueOrChunk<Vec<ExecutionResult>>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    pub(super) is_valid: OnceCell<Result<bool, bytesrepr::Error>>,
}

impl BlockExecutionResultsOrChunk {
    pub(crate) fn new(
        block_hash: BlockHash,
        chunk_index: u64,
        execution_results: Vec<ExecutionResult>,
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

        let is_v1 = matches!(execution_results.first(), Some(ExecutionResult::V1(_)));

        // If it's not V1, just construct the `ValueOrChunk` from `Vec<ExecutionResult>`.
        if !is_v1 {
            let value = make_value_or_chunk(execution_results, &block_hash, chunk_index)?;
            return Some(BlockExecutionResultsOrChunk {
                block_hash,
                value,
                is_valid: OnceCell::new(),
            });
        }

        // If it is V1, we need to construct the `ValueOrChunk` from a `Vec<ExecutionResultV1>` if
        // it's big enough to need chunking, otherwise we need to use the `Vec<ExecutionResult>` as
        // the `ValueOrChunk::Value`.
        let mut v1_results = Vec::with_capacity(execution_results.len());
        for result in &execution_results {
            if let ExecutionResult::V1(v1_result) = result {
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
            // Avoid using `make_value_or_chunk(execution_results, ..)` as that will chunk if
            // `v1_results.serialized_length() == ChunkWithProof::CHUNK_SIZE_BYTES`, since
            // `execution_results.serialized_length()` will definitely be greater than
            // `ChunkWithProof::CHUNK_SIZE_BYTES` due to the extra tag byte specifying V1 in the
            // enum `ExecutionResult`.
            let value = ValueOrChunk::Value(execution_results);
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
                    Some(ExecutionResult::V1(_))
                );
                let actual = if is_v1 {
                    let mut v1_results = Vec::with_capacity(block_execution_results.len());
                    for result in block_execution_results {
                        if let ExecutionResult::V1(v1_result) = result {
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
    pub fn into_value(self) -> ValueOrChunk<Vec<ExecutionResult>> {
        self.value
    }

    /// Returns the hash of the block this execution result belongs to.
    pub fn block_hash(&self) -> &BlockHash {
        &self.block_hash
    }

    #[cfg(test)]
    pub(crate) fn new_mock_value(rng: &mut TestRng, block_hash: BlockHash) -> Self {
        Self::new_mock_value_with_multiple_random_results(rng, block_hash, 1)
    }

    #[cfg(test)]
    pub(crate) fn new_mock_value_with_multiple_random_results(
        rng: &mut TestRng,
        block_hash: BlockHash,
        num_results: usize,
    ) -> Self {
        let execution_results: Vec<ExecutionResult> = (0..num_results)
            .map(|_| ExecutionResultV2::random(rng).into())
            .collect();

        Self {
            block_hash,
            value: ValueOrChunk::new(execution_results, 0).unwrap(),
            is_valid: OnceCell::with_value(Ok(true)),
        }
    }

    #[cfg(test)]
    pub(crate) fn value(&self) -> &ValueOrChunk<Vec<ExecutionResult>> {
        &self.value
    }

    #[cfg(test)]
    pub(crate) fn new_from_value(
        block_hash: BlockHash,
        value: ValueOrChunk<Vec<ExecutionResult>>,
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

    use casper_types::{
        execution::{execution_result_v1::ExecutionEffect, ExecutionResultV1},
        testing::TestRng,
        ChunkWithProof, TransferAddr,
    };

    use super::*;
    use crate::contract_runtime::compute_execution_results_checksum;

    fn compute_execution_results_v1_checksum(
        v1_execution_results: Vec<&ExecutionResultV1>,
    ) -> ExecutionResultsChecksum {
        ExecutionResultsChecksum::Checkable(v1_execution_results.hash().unwrap())
    }

    /// Checks that a Vec of `ExecutionResultV1`s which are right at the limit to avoid being
    /// chunked are still not chunked when constructing a BlockExecutionResultsOrChunk from them
    /// when they are held as a Vec of `ExecutionResult`s.
    #[test]
    fn should_not_chunk_for_v1_at_upper_bound() {
        let rng = &mut TestRng::new();

        // The serialized_length() of this should be equal to `ChunkWithProof::CHUNK_SIZE_BYTES`
        let execution_results_v1 = vec![ExecutionResultV1::Failure {
            effect: ExecutionEffect::default(),
            transfers: vec![TransferAddr::new([1; 32]); 262143],
            cost: 2_u64.into(),
            error_message: "ninebytes".to_string(),
        }];
        assert!(
            execution_results_v1.serialized_length() == ChunkWithProof::CHUNK_SIZE_BYTES,
            "need execution_results_v1.serialized_length() [{}] to be <= \
            ChunkWithProof::CHUNK_SIZE_BYTES [{}]",
            execution_results_v1.serialized_length(),
            ChunkWithProof::CHUNK_SIZE_BYTES
        );
        // The serialized_length() of this should be greater than `ChunkWithProof::CHUNK_SIZE_BYTES`
        // meaning it would be chunked unless we explicitly avoid chunking it in the
        // `BlockExecutionResultsOrChunk` constructor.
        let execution_results = execution_results_v1
            .iter()
            .map(|res| ExecutionResult::V1(res.clone()))
            .collect::<Vec<_>>();
        assert!(
            execution_results.serialized_length() > ChunkWithProof::CHUNK_SIZE_BYTES,
            "need execution_results.serialized_length() [{}] to be > \
            ChunkWithProof::CHUNK_SIZE_BYTES [{}]",
            execution_results_v1.serialized_length(),
            ChunkWithProof::CHUNK_SIZE_BYTES
        );
        assert!(execution_results.serialized_length() > ChunkWithProof::CHUNK_SIZE_BYTES);

        let block_hash = BlockHash::random(rng);
        let value_or_chunk =
            BlockExecutionResultsOrChunk::new(block_hash, 0, execution_results).unwrap();
        assert!(matches!(value_or_chunk.value, ValueOrChunk::Value(_)));
    }

    #[test]
    fn should_validate_v1_unchunked_checksum() {
        let rng = &mut TestRng::new();
        let execution_results = vec![
            ExecutionResult::V1(rng.gen()),
            ExecutionResult::V1(rng.gen()),
        ];
        let checksum = compute_execution_results_v1_checksum(
            execution_results
                .iter()
                .map(|exec_result| match exec_result {
                    ExecutionResult::V1(exec_result) => exec_result,
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
        let execution_results = vec![ExecutionResult::V1(v1_result); count];
        let checksum = compute_execution_results_v1_checksum(
            execution_results
                .iter()
                .map(|exec_result| match exec_result {
                    ExecutionResult::V1(exec_result) => exec_result,
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
            ExecutionResult::from(ExecutionResultV2::random(rng)),
            ExecutionResult::from(ExecutionResultV2::random(rng)),
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
        let execution_results = vec![ExecutionResult::V2(v2_result); count];
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
