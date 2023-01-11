use std::{
    collections::HashMap,
    fmt::{Debug, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use tracing::error;

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{
    bytesrepr::{self},
    ExecutionResult,
};

use crate::types::{
    BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, BlockHash, DeployHash,
    ValueOrChunk,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug, Serialize, Deserialize)]
pub(crate) enum ExecutionResultsChecksum {
    // due to historical reasons, pre-1.5 chunks do not support Merkle proof checking
    Uncheckable,
    // can be Merkle proof checked
    Checkable(Digest),
}

impl Display for ExecutionResultsChecksum {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uncheckable => write!(f, "uncheckable execution results"),
            Self::Checkable(digest) => write!(f, "execution results checksum {}", digest),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    BlockHashMismatch {
        expected: BlockHash,
        actual: BlockHash,
    },
    ChunkCountMismatch {
        block_hash: BlockHash,
        expected: u64,
        actual: u64,
    },
    InvalidChunkCount {
        block_hash: BlockHash,
    },
    InvalidAttemptToApplyChecksum {
        block_hash: BlockHash,
    },
    AttemptToApplyDataAfterCompleted {
        block_hash: BlockHash,
    },
    AttemptToApplyDataWhenMissingChecksum {
        block_hash: BlockHash,
    },
    ChecksumMismatch {
        block_hash: BlockHash,
        expected: Digest,
        actual: Digest,
    },
    ChunksWithDifferentChecksum {
        block_hash: BlockHash,
        expected: Digest,
        actual: Digest,
    },
    FailedToDeserialize {
        block_hash: BlockHash,
    },
    ExecutionResultToDeployHashLengthDiscrepancy {
        block_hash: BlockHash,
        expected: usize,
        actual: usize,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BlockHashMismatch { expected, actual } => {
                write!(
                    f,
                    "block hash mismatch: expected {} actual: {}",
                    expected, actual
                )
            }
            Error::ExecutionResultToDeployHashLengthDiscrepancy {
                block_hash,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "discrepancy between the number of deploys and corresponding execution results for block_hash: {}; expected {} actual: {}",
                    block_hash, expected, actual
                )
            }
            Error::ChunkCountMismatch {
                block_hash,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "chunk count mismatch for block_hash: {}; expected {} actual: {}",
                    block_hash, expected, actual
                )
            }
            Error::InvalidChunkCount { block_hash } => {
                write!(
                    f,
                    "invalid chunk count for block_hash: {}; execution results should either be a complete single value or come in 2 or more chunks",
                    block_hash
                )
            }
            Error::InvalidAttemptToApplyChecksum { block_hash } => {
                write!(
                    f,
                    "attempt to apply checksum to a non-pending item, block_hash: {}",
                    block_hash
                )
            }
            Error::AttemptToApplyDataAfterCompleted { block_hash } => {
                write!(
                    f,
                    "attempt to apply execution results for already completed block_hash: {}",
                    block_hash
                )
            }
            Error::AttemptToApplyDataWhenMissingChecksum { block_hash } => {
                write!(
                    f,
                    "attempt to apply execution results before check sum for block_hash: {}",
                    block_hash
                )
            }
            Error::ChecksumMismatch {
                block_hash,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "root hash mismatch for block_hash: {}; expected {} actual: {}",
                    block_hash, expected, actual
                )
            }
            Error::FailedToDeserialize { block_hash } => {
                write!(
                    f,
                    "failed to deserialize execution effects for block_hash: {}",
                    block_hash,
                )
            }
            Error::ChunksWithDifferentChecksum {
                block_hash,
                expected,
                actual,
            } => write!(
                f,
                "chunks with different checksum for block_hash: {}; expected {} actual: {}",
                block_hash, expected, actual
            ),
        }
    }
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum ExecutionResultsAcquisition {
    Needed {
        block_hash: BlockHash,
    },
    Pending {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
    },
    Acquiring {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        chunks: HashMap<u64, ChunkWithProof>,
        chunk_count: u64,
        next: u64,
    },
    Complete {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        results: HashMap<DeployHash, ExecutionResult>,
    },
}

impl ExecutionResultsAcquisition {
    pub(super) fn needs_value_or_chunk(
        &self,
    ) -> Option<(BlockExecutionResultsOrChunkId, ExecutionResultsChecksum)> {
        match self {
            ExecutionResultsAcquisition::Needed { .. }
            | ExecutionResultsAcquisition::Complete { .. } => None,
            ExecutionResultsAcquisition::Pending {
                block_hash,
                checksum,
            } => Some((BlockExecutionResultsOrChunkId::new(*block_hash), *checksum)),
            ExecutionResultsAcquisition::Acquiring {
                block_hash,
                checksum,
                next,
                ..
            } => Some((
                BlockExecutionResultsOrChunkId::new(*block_hash).next_chunk(*next),
                *checksum,
            )),
        }
    }

    pub(super) fn apply_checksum(self, checksum: ExecutionResultsChecksum) -> Result<Self, Error> {
        match self {
            ExecutionResultsAcquisition::Needed { block_hash } => {
                Ok(ExecutionResultsAcquisition::Pending {
                    block_hash,
                    checksum,
                })
            }
            ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Acquiring { block_hash, .. }
            | ExecutionResultsAcquisition::Complete { block_hash, .. } => {
                Err(Error::InvalidAttemptToApplyChecksum { block_hash })
            }
        }
    }

    pub(super) fn apply_block_execution_results_or_chunk(
        self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
        deploy_hashes: Vec<DeployHash>,
    ) -> Result<Self, Error> {
        let block_hash = *block_execution_results_or_chunk.block_hash();
        let value = block_execution_results_or_chunk.into_value();

        let expected_block_hash = self.block_hash();
        if expected_block_hash != block_hash {
            return Err(Error::BlockHashMismatch {
                expected: expected_block_hash,
                actual: block_hash,
            });
        }

        let (checksum, execution_results) = match (self, value) {
            (
                ExecutionResultsAcquisition::Pending { checksum, .. },
                ValueOrChunk::Value(execution_results),
            )
            | (
                ExecutionResultsAcquisition::Acquiring { checksum, .. },
                ValueOrChunk::Value(execution_results),
            ) => (checksum, execution_results),
            (
                ExecutionResultsAcquisition::Pending { checksum, .. },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => match apply_chunk(block_hash, checksum, HashMap::new(), chunk, None) {
                Ok(ApplyChunkOutcome::NeedNext {
                    chunks,
                    chunk_count,
                    next,
                }) => {
                    return Ok(ExecutionResultsAcquisition::Acquiring {
                        block_hash,
                        checksum,
                        chunks,
                        chunk_count,
                        next,
                    });
                }
                Ok(ApplyChunkOutcome::Complete { execution_results }) => {
                    (checksum, execution_results)
                }
                Err(err) => {
                    return Err(err);
                }
            },
            (
                ExecutionResultsAcquisition::Acquiring {
                    checksum,
                    chunks,
                    chunk_count,
                    ..
                },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => match apply_chunk(block_hash, checksum, chunks, chunk, Some(chunk_count)) {
                Ok(ApplyChunkOutcome::NeedNext {
                    chunks,
                    chunk_count,
                    next,
                }) => {
                    return Ok(ExecutionResultsAcquisition::Acquiring {
                        block_hash,
                        checksum,
                        chunks,
                        chunk_count,
                        next,
                    });
                }
                Ok(ApplyChunkOutcome::Complete { execution_results }) => {
                    (checksum, execution_results)
                }
                Err(err) => {
                    return Err(err);
                }
            },
            (ExecutionResultsAcquisition::Needed { block_hash }, _) => {
                return Err(Error::AttemptToApplyDataWhenMissingChecksum { block_hash });
            }
            (ExecutionResultsAcquisition::Complete { .. }, _) => {
                return Err(Error::AttemptToApplyDataAfterCompleted { block_hash });
            }
        };

        if deploy_hashes.len() != execution_results.len() {
            return Err(Error::ExecutionResultToDeployHashLengthDiscrepancy {
                block_hash,
                expected: deploy_hashes.len(),
                actual: execution_results.len(),
            });
        }
        let results = deploy_hashes.into_iter().zip(execution_results).collect();
        Ok(ExecutionResultsAcquisition::Complete {
            block_hash,
            results,
            checksum,
        })
    }

    pub(super) fn is_checkable(&self) -> bool {
        match self {
            ExecutionResultsAcquisition::Needed { .. } => false,
            ExecutionResultsAcquisition::Pending { checksum, .. }
            | ExecutionResultsAcquisition::Acquiring { checksum, .. }
            | ExecutionResultsAcquisition::Complete { checksum, .. } => {
                matches!(checksum, ExecutionResultsChecksum::Checkable(_))
            }
        }
    }

    fn block_hash(&self) -> BlockHash {
        match self {
            ExecutionResultsAcquisition::Needed { block_hash }
            | ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Acquiring { block_hash, .. }
            | ExecutionResultsAcquisition::Complete { block_hash, .. } => *block_hash,
        }
    }
}

enum ApplyChunkOutcome {
    NeedNext {
        chunks: HashMap<u64, ChunkWithProof>,
        chunk_count: u64,
        next: u64,
    },
    Complete {
        execution_results: Vec<ExecutionResult>,
    },
}

impl ApplyChunkOutcome {
    fn need_next(chunks: HashMap<u64, ChunkWithProof>, chunk_count: u64, next: u64) -> Self {
        ApplyChunkOutcome::NeedNext {
            chunks,
            chunk_count,
            next,
        }
    }
    fn execution_results(execution_results: Vec<ExecutionResult>) -> Self {
        ApplyChunkOutcome::Complete { execution_results }
    }
}

fn apply_chunk(
    block_hash: BlockHash,
    checksum: ExecutionResultsChecksum,
    mut chunks: HashMap<u64, ChunkWithProof>,
    chunk: ChunkWithProof,
    expected_count: Option<u64>,
) -> Result<ApplyChunkOutcome, Error> {
    let digest = chunk.proof().root_hash();
    let index = chunk.proof().index();
    let chunk_count = chunk.proof().count();
    if chunk_count == 1 {
        return Err(Error::InvalidChunkCount { block_hash });
    }

    if let Some(expected) = expected_count {
        if expected != chunk_count {
            return Err(Error::ChunkCountMismatch {
                block_hash,
                expected,
                actual: chunk_count,
            });
        }
    }

    // ExecutionResultsChecksum::Legacy has no checksum, otherwise check it
    if let ExecutionResultsChecksum::Checkable(expected) = checksum {
        if expected != digest {
            return Err(Error::ChecksumMismatch {
                block_hash,
                expected,
                actual: digest,
            });
        }
    } else if let Some(other_chunk) = chunks.values().next() {
        let existing_chunk_digest = other_chunk.proof().root_hash();
        if existing_chunk_digest != digest {
            return Err(Error::ChunksWithDifferentChecksum {
                block_hash,
                expected: existing_chunk_digest,
                actual: digest,
            });
        }
    }
    let _ = chunks.insert(index, chunk);

    match (0..chunk_count).find(|idx| !chunks.contains_key(idx)) {
        Some(next) => Ok(ApplyChunkOutcome::need_next(chunks, chunk_count, next)),
        None => {
            let serialized: Vec<u8> = (0..chunk_count)
                .filter_map(|index| chunks.get(&index))
                .flat_map(|c| c.chunk())
                .copied()
                .collect();
            match bytesrepr::deserialize(serialized) {
                Ok(results) => Ok(ApplyChunkOutcome::execution_results(results)),
                Err(error) => {
                    error!(%error, "failed to deserialize execution results");
                    Err(Error::FailedToDeserialize { block_hash })
                }
            }
        }
    }
}
