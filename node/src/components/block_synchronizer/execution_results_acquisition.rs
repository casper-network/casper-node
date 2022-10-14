use std::{
    collections::HashMap,
    fmt::{Debug, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use tracing::error;

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::bytesrepr::{self};

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
    InvalidAttemptToApplyDeployHashes {
        block_hash: BlockHash,
    },
    AttemptToApplyDataAfterCompleted {
        block_hash: BlockHash,
    },
    AttemptToApplyDataWhenUnneeded {
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
            Error::InvalidAttemptToApplyDeployHashes { block_hash } => {
                write!(
                    f,
                    "attempt to apply deploy_hashes in a state other than Complete, block_hash: {}",
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
            Error::AttemptToApplyDataWhenUnneeded { block_hash } => {
                write!(
                    f,
                    "attempt to apply unneeded execution results for block_hash: {}",
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
    Unneeded {
        block_hash: BlockHash,
    },
    Needed {
        block_hash: BlockHash,
    },
    Pending {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
    },
    Incomplete {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        chunks: HashMap<u64, ChunkWithProof>,
        chunk_count: u64,
        next: u64,
    },
    Complete {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        results: Vec<casper_types::ExecutionResult>,
    },
    Mapped {
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        results: HashMap<DeployHash, casper_types::ExecutionResult>,
    },
}

impl ExecutionResultsAcquisition {
    pub(super) fn new(block_hash: BlockHash, checksum: ExecutionResultsChecksum) -> Self {
        Self::Pending {
            block_hash,
            checksum,
        }
    }

    pub(super) fn needs_value_or_chunk(
        &self,
    ) -> Option<(BlockExecutionResultsOrChunkId, ExecutionResultsChecksum)> {
        if let Some(chunk_id) = self.first_missing() {
            if let Some((ret, checksum)) = self.execution_results_or_chunk_id() {
                // todo!() Isn't chunk_id already the same as in ret?
                return Some((ret.next_chunk(chunk_id), checksum));
            }
        }
        None
    }

    pub(super) fn apply_checksum(
        mut self,
        checksum: ExecutionResultsChecksum,
    ) -> Result<Self, Error> {
        match self {
            ExecutionResultsAcquisition::Needed { block_hash } => {
                Ok(ExecutionResultsAcquisition::Pending {
                    block_hash,
                    checksum,
                })
            }
            ExecutionResultsAcquisition::Unneeded { block_hash, .. }
            | ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Incomplete { block_hash, .. }
            | ExecutionResultsAcquisition::Complete { block_hash, .. }
            | ExecutionResultsAcquisition::Mapped { block_hash, .. } => {
                Err(Error::InvalidAttemptToApplyChecksum { block_hash })
            }
        }
    }

    pub(super) fn apply_block_execution_results_or_chunk(
        self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
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

        match (self, value) {
            (ExecutionResultsAcquisition::Unneeded { block_hash }, _) => {
                Err(Error::AttemptToApplyDataWhenUnneeded { block_hash })
            }
            (ExecutionResultsAcquisition::Needed { block_hash }, _) => {
                Err(Error::AttemptToApplyDataWhenMissingChecksum { block_hash })
            }
            (ExecutionResultsAcquisition::Complete { .. }, _)
            | (ExecutionResultsAcquisition::Mapped { .. }, _) => {
                Err(Error::AttemptToApplyDataAfterCompleted { block_hash })
            }
            (
                ExecutionResultsAcquisition::Pending { checksum, .. },
                ValueOrChunk::Value(results),
            )
            | (
                ExecutionResultsAcquisition::Incomplete { checksum, .. },
                ValueOrChunk::Value(results),
            ) => Ok(Self::Complete {
                block_hash,
                checksum,
                results,
            }),
            (
                ExecutionResultsAcquisition::Pending { checksum, .. },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => apply_chunk(block_hash, checksum, HashMap::new(), chunk, None),
            (
                ExecutionResultsAcquisition::Incomplete {
                    checksum,
                    chunks,
                    chunk_count,
                    ..
                },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => apply_chunk(block_hash, checksum, chunks, chunk, Some(chunk_count)),
        }
    }

    // todo!() - not hooked
    pub(super) fn apply_deploy_hashes(self, deploy_hashes: Vec<DeployHash>) -> Result<Self, Error> {
        match self {
            ExecutionResultsAcquisition::Unneeded { block_hash, .. }
            | ExecutionResultsAcquisition::Needed { block_hash, .. }
            | ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Incomplete { block_hash, .. }
            | ExecutionResultsAcquisition::Mapped { block_hash, .. } => {
                Err(Error::InvalidAttemptToApplyDeployHashes { block_hash })
            }
            ExecutionResultsAcquisition::Complete {
                block_hash,
                checksum,
                results,
                ..
            } => {
                if deploy_hashes.len() != results.len() {
                    return Err(Error::ExecutionResultToDeployHashLengthDiscrepancy {
                        block_hash,
                        expected: deploy_hashes.len(),
                        actual: results.len(),
                    });
                }
                let ret = deploy_hashes.into_iter().zip(results).collect();
                Ok(Self::Mapped {
                    block_hash,
                    checksum,
                    results: ret,
                })
            }
        }
    }

    fn block_hash(&self) -> BlockHash {
        match self {
            ExecutionResultsAcquisition::Unneeded { block_hash }
            | ExecutionResultsAcquisition::Needed { block_hash }
            | ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Incomplete { block_hash, .. }
            | ExecutionResultsAcquisition::Complete { block_hash, .. }
            | ExecutionResultsAcquisition::Mapped { block_hash, .. } => *block_hash,
        }
    }

    fn first_missing(&self) -> Option<u64> {
        match self {
            ExecutionResultsAcquisition::Needed { .. }
            | ExecutionResultsAcquisition::Pending { .. } => Some(0),
            ExecutionResultsAcquisition::Incomplete { next, .. } => Some(*next),
            ExecutionResultsAcquisition::Complete { .. }
            | ExecutionResultsAcquisition::Mapped { .. }
            | ExecutionResultsAcquisition::Unneeded { .. } => None,
        }
    }

    fn execution_results_or_chunk_id(
        &self,
    ) -> Option<(BlockExecutionResultsOrChunkId, ExecutionResultsChecksum)> {
        match self {
            ExecutionResultsAcquisition::Unneeded { .. }
            | ExecutionResultsAcquisition::Needed { .. }
            | ExecutionResultsAcquisition::Complete { .. }
            | ExecutionResultsAcquisition::Mapped { .. } => None,
            ExecutionResultsAcquisition::Pending {
                block_hash,
                checksum,
            } => Some((BlockExecutionResultsOrChunkId::new(*block_hash), *checksum)),
            ExecutionResultsAcquisition::Incomplete {
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
}

fn apply_chunk(
    block_hash: BlockHash,
    checksum: ExecutionResultsChecksum,
    mut chunks: HashMap<u64, ChunkWithProof>,
    chunk: ChunkWithProof,
    expected_count: Option<u64>,
) -> Result<ExecutionResultsAcquisition, Error> {
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

    // ExecutionResultsRootHash::Legacy has no checksum, otherwise check it
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
        Some(next) => Ok(ExecutionResultsAcquisition::Incomplete {
            block_hash,
            checksum,
            chunks,
            chunk_count,
            next,
        }),
        None => {
            let serialized: Vec<u8> = (0..chunk_count)
                .filter_map(|index| chunks.get(&index))
                .flat_map(|c| c.chunk())
                .copied()
                .collect();
            match bytesrepr::deserialize(serialized) {
                Ok(results) => Ok(ExecutionResultsAcquisition::Complete {
                    block_hash,
                    checksum,
                    results,
                }),
                Err(error) => {
                    error!(%error, "failed to deserialize execution results");
                    Err(Error::FailedToDeserialize { block_hash })
                }
            }
        }
    }
}
