use std::{
    collections::HashMap,
    fmt::{self, Debug, Display, Formatter},
};

use casper_execution_engine::storage::trie::TrieRaw;
use datasize::DataSize;

use tracing::debug;

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::bytesrepr::Bytes;

use crate::types::{TrieOrChunk, TrieOrChunkId, ValueOrChunk};

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    TrieHashMismatch {
        expected: Digest,
        actual: Digest,
    },
    ChunkCountMismatch {
        trie_hash: Digest,
        expected: u64,
        actual: u64,
    },
    InvalidChunkCount {
        trie_hash: Digest,
    },
    AttemptToApplyDataAfterCompleted {
        trie_hash: Digest,
    },
    ChunksWithDifferentRootHashes {
        trie_hash: Digest,
        expected: Digest,
        actual: Digest,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TrieHashMismatch { expected, actual } => {
                write!(
                    f,
                    "trie hash mismatch: expected {} actual: {}",
                    expected, actual
                )
            }
            Error::ChunkCountMismatch {
                trie_hash,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "chunk count mismatch for trie_hash: {}; expected {} actual: {}",
                    trie_hash, expected, actual
                )
            }
            Error::InvalidChunkCount { trie_hash } => {
                write!(
                    f,
                    "invalid chunk count for trie_hash: {}; trie should either be a complete single value or come in 2 or more chunks",
                    trie_hash
                )
            }
            Error::AttemptToApplyDataAfterCompleted { trie_hash } => {
                write!(
                    f,
                    "attempt to apply trie or chunks for already completed trie_hash: {}",
                    trie_hash
                )
            }
            Error::ChunksWithDifferentRootHashes {
                trie_hash,
                expected,
                actual,
            } => write!(
                f,
                "chunks with different root hashes for trie_hash: {}; expected {} actual: {}",
                trie_hash, expected, actual
            ),
        }
    }
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum TrieAcquisition {
    Needed {
        trie_hash: Digest,
    },
    Acquiring {
        trie_hash: Digest,
        chunks: HashMap<u64, ChunkWithProof>,
        chunk_count: u64,
        next: u64,
    },
    Complete {
        trie_hash: Digest,
        trie: Box<TrieRaw>,
    },
}

impl Display for TrieAcquisition {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TrieAcquisition::Needed { trie_hash } => {
                write!(f, "Needed: {}", trie_hash)
            }
            TrieAcquisition::Acquiring {
                trie_hash,
                chunks: _,
                chunk_count,
                next,
            } => write!(
                f,
                "Acquiring: {}, chunk_count={}, next={}",
                trie_hash, chunk_count, next
            ),
            TrieAcquisition::Complete { trie_hash, trie: _ } => {
                write!(f, "Complete: {}", trie_hash)
            }
        }
    }
}

impl TrieAcquisition {
    pub(super) fn needs_value_or_chunk(&self) -> Option<TrieOrChunkId> {
        match self {
            TrieAcquisition::Needed { trie_hash } => Some(TrieOrChunkId::new(0, *trie_hash)),
            TrieAcquisition::Acquiring {
                trie_hash, next, ..
            } => Some(TrieOrChunkId::new(*next, *trie_hash)),
            TrieAcquisition::Complete { .. } => None,
        }
    }

    pub(super) fn apply_trie_or_chunk(self, trie_or_chunk: TrieOrChunk) -> Result<Self, Error> {
        let trie_hash = *trie_or_chunk.trie_hash();
        let value = trie_or_chunk.into_value();

        debug!(%trie_hash, state=%self, "apply_trie_or_chunk");

        let expected_trie_hash = self.trie_hash();
        if expected_trie_hash != trie_hash {
            debug!(
                %trie_hash,
                "apply_trie_or_chunk: Error::TrieHashMismatch"
            );
            return Err(Error::TrieHashMismatch {
                expected: expected_trie_hash,
                actual: trie_hash,
            });
        }

        match (self, value) {
            (TrieAcquisition::Acquiring { .. }, ValueOrChunk::Value(trie))
            | (TrieAcquisition::Needed { .. }, ValueOrChunk::Value(trie)) => {
                debug!("apply_trie_or_chunk: (Pending, Value) | (Acquiring, Value)");
                Ok(TrieAcquisition::Complete {
                    trie_hash,
                    trie: Box::new(trie.into_inner()),
                })
            }
            (TrieAcquisition::Needed { .. }, ValueOrChunk::ChunkWithProof(chunk)) => {
                debug!("apply_trie_or_chunk: (Needed, ChunkWithProof)");
                match apply_chunk(trie_hash, HashMap::new(), chunk, None) {
                    Ok(ApplyChunkOutcome::NeedNext {
                        chunks,
                        chunk_count,
                        next,
                    }) => Ok(TrieAcquisition::Acquiring {
                        trie_hash,
                        chunks,
                        chunk_count,
                        next,
                    }),
                    Ok(ApplyChunkOutcome::Complete { trie }) => {
                        Ok(TrieAcquisition::Complete { trie_hash, trie })
                    }
                    Err(err) => Err(err),
                }
            }
            (
                TrieAcquisition::Acquiring {
                    chunks,
                    chunk_count,
                    ..
                },
                ValueOrChunk::ChunkWithProof(chunk),
            ) => {
                debug!("apply_trie_or_chunk: (Acquiring, ChunkWithProof)");
                match apply_chunk(trie_hash, chunks, chunk, Some(chunk_count)) {
                    Ok(ApplyChunkOutcome::NeedNext {
                        chunks,
                        chunk_count,
                        next,
                    }) => Ok(TrieAcquisition::Acquiring {
                        trie_hash,
                        chunks,
                        chunk_count,
                        next,
                    }),
                    Ok(ApplyChunkOutcome::Complete { trie }) => {
                        Ok(TrieAcquisition::Complete { trie_hash, trie })
                    }
                    Err(err) => Err(err),
                }
            }
            (TrieAcquisition::Complete { .. }, _) => {
                debug!("apply_trie_or_chunk: (Complete, _)");
                Err(Error::AttemptToApplyDataAfterCompleted { trie_hash })
            }
        }
    }

    pub(super) fn trie_hash(&self) -> Digest {
        match self {
            TrieAcquisition::Needed { trie_hash }
            | TrieAcquisition::Acquiring { trie_hash, .. }
            | TrieAcquisition::Complete { trie_hash, .. } => *trie_hash,
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
        trie: Box<TrieRaw>,
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
    fn trie(trie: Box<TrieRaw>) -> Self {
        ApplyChunkOutcome::Complete { trie }
    }
}

fn apply_chunk(
    trie_hash: Digest,
    mut chunks: HashMap<u64, ChunkWithProof>,
    chunk: ChunkWithProof,
    expected_count: Option<u64>,
) -> Result<ApplyChunkOutcome, Error> {
    let digest = chunk.proof().root_hash();
    let index = chunk.proof().index();
    let chunk_count = chunk.proof().count();
    if chunk_count == 1 {
        debug!(%trie_hash, "apply_chunk: Error::InvalidChunkCount");
        return Err(Error::InvalidChunkCount { trie_hash });
    }

    if let Some(expected) = expected_count {
        if expected != chunk_count {
            debug!(%trie_hash, "apply_chunk: Error::ChunkCountMismatch");
            return Err(Error::ChunkCountMismatch {
                trie_hash,
                expected,
                actual: chunk_count,
            });
        }
    }

    if let Some(other_chunk) = chunks.values().next() {
        let existing_chunk_digest = other_chunk.proof().root_hash();
        if existing_chunk_digest != digest {
            debug!(%trie_hash, "apply_chunk: Error::ChunksWithDifferentRootHashes");
            return Err(Error::ChunksWithDifferentRootHashes {
                trie_hash,
                expected: existing_chunk_digest,
                actual: digest,
            });
        }
    }
    let _ = chunks.insert(index, chunk);

    match (0..chunk_count).find(|idx| !chunks.contains_key(idx)) {
        Some(next) => Ok(ApplyChunkOutcome::need_next(chunks, chunk_count, next)),
        None => {
            let data: Bytes = (0..chunk_count)
                .filter_map(|index| chunks.get(&index))
                .flat_map(|chunk| chunk.chunk())
                .copied()
                .collect();
            Ok(ApplyChunkOutcome::trie(Box::new(TrieRaw::new(data))))
        }
    }
}
