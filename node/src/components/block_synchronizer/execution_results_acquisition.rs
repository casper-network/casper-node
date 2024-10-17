#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use casper_types::{
    bytesrepr, execution::ExecutionResult, BlockHash, ChunkWithProof, Digest, TransactionHash,
};

use super::block_acquisition::Acceptance;
use crate::types::{BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, ValueOrChunk};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug, Serialize, Deserialize)]
pub(crate) enum ExecutionResultsChecksum {
    // due to historical reasons, pre-1.5 chunks do not support Merkle proof checking
    Uncheckable,
    // can be Merkle proof checked
    Checkable(Digest),
}

impl Display for ExecutionResultsChecksum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncheckable => write!(f, "uncheckable execution results"),
            Self::Checkable(digest) => write!(f, "execution results checksum {}", digest),
        }
    }
}

impl ExecutionResultsChecksum {
    pub(super) fn is_checkable(&self) -> bool {
        matches!(self, ExecutionResultsChecksum::Checkable(_))
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
    InvalidOutcomeFromApplyingChunk {
        block_hash: BlockHash,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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
            Error::InvalidOutcomeFromApplyingChunk { block_hash } => write!(
                f,
                "cannot have already had chunk if in pending mode for block hash: {}",
                block_hash
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
        results: HashMap<TransactionHash, ExecutionResult>,
    },
}

impl Display for ExecutionResultsAcquisition {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionResultsAcquisition::Needed { block_hash } => {
                write!(f, "Needed: {}", block_hash)
            }
            ExecutionResultsAcquisition::Pending {
                block_hash,
                checksum: _,
            } => write!(f, "Pending: {}", block_hash),
            ExecutionResultsAcquisition::Acquiring {
                block_hash,
                checksum: _,
                chunks: _,
                chunk_count,
                next,
            } => write!(
                f,
                "Acquiring: {}, chunk_count={}, next={}",
                block_hash, chunk_count, next
            ),
            ExecutionResultsAcquisition::Complete {
                block_hash,
                checksum: _,
                results: _,
            } => write!(f, "Complete: {}", block_hash),
        }
    }
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
                debug!("apply_checksum - Needed");
                Ok(ExecutionResultsAcquisition::Pending {
                    block_hash,
                    checksum,
                })
            }
            ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Acquiring { block_hash, .. }
            | ExecutionResultsAcquisition::Complete { block_hash, .. } => {
                debug!("apply_checksum - Pending | Acquiring | Complete");
                Err(Error::InvalidAttemptToApplyChecksum { block_hash })
            }
        }
    }

    pub(super) fn apply_block_execution_results_or_chunk(
        self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
        transaction_hashes: Vec<TransactionHash>,
    ) -> Result<(Self, Acceptance), Error> {
        let block_hash = *block_execution_results_or_chunk.block_hash();
        let value = block_execution_results_or_chunk.into_value();

        debug!(%block_hash, state=%self, "apply_block_execution_results_or_chunk");

        let expected_block_hash = self.block_hash();
        if expected_block_hash != block_hash {
            debug!(
                %block_hash,
                "apply_block_execution_results_or_chunk: Error::BlockHashMismatch"
            );
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
            ) => {
                debug!(
                    "apply_block_execution_results_or_chunk: (Pending, Value) | (Acquiring, Value)"
                );
                (checksum, execution_results)
            }
            (
                ExecutionResultsAcquisition::Pending { checksum, .. },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => {
                debug!("apply_block_execution_results_or_chunk: (Pending, ChunkWithProof)");
                match apply_chunk(block_hash, checksum, HashMap::new(), chunk, None) {
                    Ok(ApplyChunkOutcome::HadIt { .. }) => {
                        error!("cannot have already had chunk if in pending mode");
                        return Err(Error::InvalidOutcomeFromApplyingChunk { block_hash });
                    }
                    Ok(ApplyChunkOutcome::NeedNext {
                        chunks,
                        chunk_count,
                        next,
                    }) => {
                        let acquisition = ExecutionResultsAcquisition::Acquiring {
                            block_hash,
                            checksum,
                            chunks,
                            chunk_count,
                            next,
                        };
                        let acceptance = Acceptance::NeededIt;
                        return Ok((acquisition, acceptance));
                    }
                    Ok(ApplyChunkOutcome::Complete { execution_results }) => {
                        (checksum, execution_results)
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }
            (
                ExecutionResultsAcquisition::Acquiring {
                    checksum,
                    chunks,
                    chunk_count,
                    next,
                    ..
                },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => {
                debug!("apply_block_execution_results_or_chunk: (Acquiring, ChunkWithProof)");
                match apply_chunk(block_hash, checksum, chunks, chunk, Some(chunk_count)) {
                    Ok(ApplyChunkOutcome::HadIt { chunks }) => {
                        let acquisition = ExecutionResultsAcquisition::Acquiring {
                            block_hash,
                            checksum,
                            chunks,
                            chunk_count,
                            next,
                        };
                        let acceptance = Acceptance::HadIt;
                        return Ok((acquisition, acceptance));
                    }
                    Ok(ApplyChunkOutcome::NeedNext {
                        chunks,
                        chunk_count,
                        next,
                    }) => {
                        let acquisition = ExecutionResultsAcquisition::Acquiring {
                            block_hash,
                            checksum,
                            chunks,
                            chunk_count,
                            next,
                        };
                        let acceptance = Acceptance::NeededIt;
                        return Ok((acquisition, acceptance));
                    }
                    Ok(ApplyChunkOutcome::Complete { execution_results }) => {
                        (checksum, execution_results)
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }
            (ExecutionResultsAcquisition::Needed { block_hash }, _) => {
                debug!("apply_block_execution_results_or_chunk: (Needed, _)");
                return Err(Error::AttemptToApplyDataWhenMissingChecksum { block_hash });
            }
            (ExecutionResultsAcquisition::Complete { .. }, _) => {
                debug!("apply_block_execution_results_or_chunk: (Complete, _)");
                return Err(Error::AttemptToApplyDataAfterCompleted { block_hash });
            }
        };

        if transaction_hashes.len() != execution_results.len() {
            debug!(
                %block_hash,
                "apply_block_execution_results_or_chunk: Error::ExecutionResultToDeployHashLengthDiscrepancy"
            );
            return Err(Error::ExecutionResultToDeployHashLengthDiscrepancy {
                block_hash,
                expected: transaction_hashes.len(),
                actual: execution_results.len(),
            });
        }
        let results = transaction_hashes
            .into_iter()
            .zip(execution_results)
            .collect();
        debug!(
            %block_hash,
            "apply_block_execution_results_or_chunk: returning ExecutionResultsAcquisition::Complete"
        );
        let acceptance = Acceptance::NeededIt;
        let acquisition = ExecutionResultsAcquisition::Complete {
            block_hash,
            results,
            checksum,
        };
        Ok((acquisition, acceptance))
    }

    pub(super) fn is_checkable(&self) -> bool {
        match self {
            ExecutionResultsAcquisition::Needed { .. } => false,
            ExecutionResultsAcquisition::Pending { checksum, .. }
            | ExecutionResultsAcquisition::Acquiring { checksum, .. }
            | ExecutionResultsAcquisition::Complete { checksum, .. } => checksum.is_checkable(),
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

#[derive(Debug)]
enum ApplyChunkOutcome {
    HadIt {
        chunks: HashMap<u64, ChunkWithProof>,
    },
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
        debug!(%block_hash, "apply_chunk: Error::InvalidChunkCount");
        return Err(Error::InvalidChunkCount { block_hash });
    }

    if let Some(expected) = expected_count {
        if expected != chunk_count {
            debug!(%block_hash, "apply_chunk: Error::ChunkCountMismatch");
            return Err(Error::ChunkCountMismatch {
                block_hash,
                expected,
                actual: chunk_count,
            });
        }
    }

    // ExecutionResultsChecksum::Uncheckable has no checksum, otherwise check it
    if let ExecutionResultsChecksum::Checkable(expected) = checksum {
        if expected != digest {
            debug!(%block_hash, "apply_chunk: Error::ChecksumMismatch");
            return Err(Error::ChecksumMismatch {
                block_hash,
                expected,
                actual: digest,
            });
        }
    } else if let Some(other_chunk) = chunks.values().next() {
        let existing_chunk_digest = other_chunk.proof().root_hash();
        if existing_chunk_digest != digest {
            debug!(%block_hash, "apply_chunk: Error::ChunksWithDifferentChecksum");
            return Err(Error::ChunksWithDifferentChecksum {
                block_hash,
                expected: existing_chunk_digest,
                actual: digest,
            });
        }
    }

    if chunks.insert(index, chunk).is_some() {
        debug!(%block_hash, index, "apply_chunk: already had it");
        return Ok(ApplyChunkOutcome::HadIt { chunks });
    };

    match (0..chunk_count).find(|idx| !chunks.contains_key(idx)) {
        Some(next) => Ok(ApplyChunkOutcome::need_next(chunks, chunk_count, next)),
        None => {
            let serialized: Vec<u8> = (0..chunk_count)
                .filter_map(|index| chunks.get(&index))
                .flat_map(|c| c.chunk())
                .copied()
                .collect();
            match bytesrepr::deserialize(serialized) {
                Ok(results) => {
                    debug!(%block_hash, "apply_chunk: ApplyChunkOutcome::execution_results");
                    Ok(ApplyChunkOutcome::execution_results(results))
                }
                Err(error) => {
                    error!(%error, "failed to deserialize execution results");
                    Err(Error::FailedToDeserialize { block_hash })
                }
            }
        }
    }
}
