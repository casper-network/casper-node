use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use casper_hashing::{ChunkWithProofVerificationError, Digest};

use super::{value_or_chunk::HashingTrieRaw, ChunkingError, ValueOrChunk};
use crate::{
    components::fetcher::{EmptyValidationMetadata, FetchItem, Tag},
    utils::ds,
};

/// Represents the ID of a `TrieOrChunk` - containing the index and the root hash.
/// The root hash is the hash of the trie node as a whole.
/// The index is the index of a chunk if the node's size is too large and requires chunking. For
/// small nodes, it's always 0.
#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TrieOrChunkId {
    pub chunk_index: u64,
    pub trie_hash: Digest,
}

impl TrieOrChunkId {
    pub fn new(chunk_index: u64, trie_hash: Digest) -> Self {
        Self {
            chunk_index,
            trie_hash,
        }
    }

    /// Returns the trie key part of the ID.
    pub fn digest(&self) -> &Digest {
        &self.trie_hash
    }

    /// Given a serialized ID, deserializes it for display purposes.
    fn fmt_serialized(f: &mut Formatter, serialized_id: &[u8]) -> fmt::Result {
        match bincode::deserialize::<Self>(serialized_id) {
            Ok(ref trie_or_chunk_id) => Display::fmt(trie_or_chunk_id, f),
            Err(_) => f.write_str("<invalid>"),
        }
    }
}

/// Helper struct to on-demand deserialize a trie or chunk ID for display purposes.
pub struct TrieOrChunkIdDisplay<'a>(pub &'a [u8]);

impl<'a> Display for TrieOrChunkIdDisplay<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        TrieOrChunkId::fmt_serialized(f, self.0)
    }
}

impl Display for TrieOrChunkId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.chunk_index, self.trie_hash)
    }
}

#[derive(thiserror::Error, Debug, DataSize, Clone, PartialEq, Eq)]
pub enum TrieOrChunkValidationError {
    #[error(transparent)]
    ChunkWithProofVerification(ChunkWithProofVerificationError),
    #[error("Trie or chunk root hash mismatch; expected: {expected}; actual: {actual}")]
    RootHashMismatch { actual: Digest, expected: Digest },
}

#[derive(Clone, Serialize, Deserialize, Debug, Eq, DataSize)]
pub struct TrieOrChunk {
    trie_hash: Digest,
    value: ValueOrChunk<HashingTrieRaw>,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    is_valid: OnceCell<Result<(), TrieOrChunkValidationError>>,
}

impl TrieOrChunk {
    pub fn new(
        trie_hash: Digest,
        trie_raw: HashingTrieRaw,
        chunk_index: u64,
    ) -> Result<TrieOrChunk, ChunkingError> {
        let value = ValueOrChunk::<HashingTrieRaw>::new(trie_raw, chunk_index)?;

        Ok(TrieOrChunk {
            trie_hash,
            value,
            is_valid: OnceCell::new(),
        })
    }

    pub fn trie_hash(&self) -> &Digest {
        &self.trie_hash
    }

    /// Consumes `self` and returns inner `ValueOrChunk` field.
    pub fn into_value(self) -> ValueOrChunk<HashingTrieRaw> {
        self.value
    }
}

impl PartialEq for TrieOrChunk {
    fn eq(&self, other: &TrieOrChunk) -> bool {
        // Destructure to make sure we don't accidentally omit fields.
        let TrieOrChunk {
            trie_hash,
            value,
            is_valid: _,
        } = self;
        *trie_hash == other.trie_hash && *value == other.value
    }
}

impl Display for TrieOrChunk {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "trie or chunk for trie hash {}", self.trie_hash)
    }
}

impl FetchItem for TrieOrChunk {
    type Id = TrieOrChunkId;
    type ValidationError = TrieOrChunkValidationError;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::TrieOrChunk;

    fn fetch_id(&self) -> Self::Id {
        match self.value {
            ValueOrChunk::Value(ref trie_raw) => TrieOrChunkId::new(0, trie_raw.hash()),
            ValueOrChunk::ChunkWithProof(ref chunked_data) => TrieOrChunkId::new(
                chunked_data.proof().index(),
                chunked_data.proof().root_hash(),
            ),
        }
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.is_valid
            .get_or_init(|| match self.value {
                ValueOrChunk::Value(ref trie) => {
                    if trie.hash() == self.trie_hash {
                        Ok(())
                    } else {
                        Err(TrieOrChunkValidationError::RootHashMismatch {
                            actual: trie.hash(),
                            expected: self.trie_hash,
                        })
                    }
                }
                ValueOrChunk::ChunkWithProof(ref chunk_with_proof) => {
                    match chunk_with_proof.verify() {
                        Ok(()) => {
                            if chunk_with_proof.proof().root_hash() == self.trie_hash {
                                Ok(())
                            } else {
                                Err(TrieOrChunkValidationError::RootHashMismatch {
                                    actual: chunk_with_proof.proof().root_hash(),
                                    expected: self.trie_hash,
                                })
                            }
                        }
                        Err(e) => Err(TrieOrChunkValidationError::ChunkWithProofVerification(e)),
                    }
                }
            })
            .clone()
    }
}
