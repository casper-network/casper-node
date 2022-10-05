use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use tracing::{debug, error, warn};

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::bytesrepr::{self};

use crate::{
    components::{
        fetcher::{FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::PeerBehaviorAnnouncement, requests::FetcherRequest, EffectBuilder,
        EffectExt, Effects, Responder,
    },
    types::{
        BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, BlockHash, DeployHash, Item,
        NodeId, ValueOrChunk,
    },
    NodeRng,
};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(super) enum ExecutionResultsChecksum {
    // due to historical reasons, pre-1.5 chunks do not support merkle proof checking
    Uncheckable,
    // can be merkle proof checked
    Checkable(Digest),
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
    InvalidAttemptToApployDeployHashes {
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
                    "attempt to apply check_sum to a non-pending item, block_hash: {}",
                    block_hash
                )
            }
            Error::InvalidAttemptToApployDeployHashes { block_hash } => {
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
        check_sum: ExecutionResultsChecksum,
    },
    Incomplete {
        block_hash: BlockHash,
        check_sum: ExecutionResultsChecksum,
        chunks: HashMap<u64, ChunkWithProof>,
        chunk_count: u64,
        next: u64,
    },
    Complete {
        block_hash: BlockHash,
        check_sum: ExecutionResultsChecksum,
        results: Vec<casper_types::ExecutionResult>,
    },
    Mapped {
        block_hash: BlockHash,
        check_sum: ExecutionResultsChecksum,
        results: HashMap<DeployHash, casper_types::ExecutionResult>,
    },
}

impl ExecutionResultsAcquisition {
    pub(super) fn new(block_hash: BlockHash, check_sum: ExecutionResultsChecksum) -> Self {
        Self::Pending {
            block_hash,
            check_sum,
        }
    }

    pub(super) fn needs_value_or_chunk(&self) -> Option<BlockExecutionResultsOrChunkId> {
        if let Some(chunk_id) = self.first_missing() {
            if let Some(ret) = self.execution_results_or_chunk_id() {
                return Some(ret.next_chunk(chunk_id));
            }
        }
        None
    }

    pub(super) fn apply_check_sum(
        mut self,
        check_sum: ExecutionResultsChecksum,
    ) -> Result<Self, Error> {
        match self {
            ExecutionResultsAcquisition::Needed { block_hash } => {
                Ok(ExecutionResultsAcquisition::Pending {
                    block_hash,
                    check_sum,
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
        mut self,
        block_execution_results_or_chunk: BlockExecutionResultsOrChunk,
    ) -> Result<Self, Error> {
        let (block_hash, value) = match block_execution_results_or_chunk {
            BlockExecutionResultsOrChunk::Legacy { block_hash, value } => (block_hash, value),
            BlockExecutionResultsOrChunk::Contemporary { block_hash, value } => (block_hash, value),
        };

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
                ExecutionResultsAcquisition::Pending { check_sum, .. },
                ValueOrChunk::Value(results),
            )
            | (
                ExecutionResultsAcquisition::Incomplete { check_sum, .. },
                ValueOrChunk::Value(results),
            ) => Ok(Self::Complete {
                block_hash,
                check_sum,
                results,
            }),
            (
                ExecutionResultsAcquisition::Pending { check_sum, .. },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => apply_chunk(block_hash, check_sum, HashMap::new(), chunk, None),
            (
                ExecutionResultsAcquisition::Incomplete {
                    check_sum,
                    chunks,
                    chunk_count,
                    ..
                },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => apply_chunk(block_hash, check_sum, chunks, chunk, Some(chunk_count)),
        }
    }

    pub(super) fn apply_deploy_hashes(
        mut self,
        deploy_hashes: Vec<DeployHash>,
    ) -> Result<Self, Error> {
        match self {
            ExecutionResultsAcquisition::Unneeded { block_hash, .. }
            | ExecutionResultsAcquisition::Needed { block_hash, .. }
            | ExecutionResultsAcquisition::Pending { block_hash, .. }
            | ExecutionResultsAcquisition::Incomplete { block_hash, .. }
            | ExecutionResultsAcquisition::Mapped { block_hash, .. } => {
                Err(Error::InvalidAttemptToApployDeployHashes { block_hash })
            }
            ExecutionResultsAcquisition::Complete {
                block_hash,
                check_sum,
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
                    check_sum,
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

    fn execution_results_or_chunk_id(&self) -> Option<BlockExecutionResultsOrChunkId> {
        match self {
            ExecutionResultsAcquisition::Unneeded { .. }
            | ExecutionResultsAcquisition::Needed { .. }
            | ExecutionResultsAcquisition::Complete { .. }
            | ExecutionResultsAcquisition::Mapped { .. } => None,
            ExecutionResultsAcquisition::Pending {
                block_hash,
                check_sum,
            } => match check_sum {
                ExecutionResultsChecksum::Uncheckable => {
                    Some(BlockExecutionResultsOrChunkId::legacy(*block_hash))
                }
                ExecutionResultsChecksum::Checkable(_) => {
                    Some(BlockExecutionResultsOrChunkId::new(*block_hash))
                }
            },
            ExecutionResultsAcquisition::Incomplete {
                block_hash,
                check_sum,
                next,
                ..
            } => match check_sum {
                ExecutionResultsChecksum::Uncheckable => {
                    Some(BlockExecutionResultsOrChunkId::legacy(*block_hash))
                }
                ExecutionResultsChecksum::Checkable(_) => {
                    Some(BlockExecutionResultsOrChunkId::Contemporary {
                        block_hash: *block_hash,
                        chunk_index: *next,
                    })
                }
            },
        }
    }
}

fn apply_chunk(
    block_hash: BlockHash,
    check_sum: ExecutionResultsChecksum,
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

    // ExecutionResultsRootHash::Legacy has no check_sum, otherwise check it
    if let ExecutionResultsChecksum::Checkable(expected) = check_sum {
        if expected != digest {
            return Err(Error::ChecksumMismatch {
                block_hash,
                expected,
                actual: digest,
            });
        }
    }
    let _ = chunks.insert(index, chunk);

    match (0..chunk_count).find(|idx| !chunks.contains_key(idx)) {
        Some(next) => Ok(ExecutionResultsAcquisition::Incomplete {
            block_hash,
            check_sum,
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
                Ok(results) => {
                    // todo!() - check merkle root - sure, but how?
                    Ok(ExecutionResultsAcquisition::Complete {
                        block_hash,
                        check_sum,
                        results,
                    })
                }
                Err(error) => {
                    error!(%error, "failed to deserialize execution results");
                    Err(Error::FailedToDeserialize { block_hash })
                }
            }
        }
    }
}
