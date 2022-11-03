use std::fmt::{self, Debug, Display, Formatter};

use casper_execution_engine::storage::trie::TrieRaw;
use casper_types::ExecutionResult;
use datasize::DataSize;
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_hashing::{
    ChunkWithProof, ChunkWithProofVerificationError, Digest, MerkleConstructionError,
};

use super::Chunkable;
use crate::types::{EmptyValidationMetadata, FetcherItem, Item, Tag};

/// Represents a value or a chunk of data with attached proof.
///
/// Chunk with attached proof is used when the requested
/// value is larger than [ChunkWithProof::CHUNK_SIZE_BYTES].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DataSize)]
pub enum ValueOrChunk<V> {
    /// Represents a value.
    Value(V),
    /// Represents a chunk of data with attached proof.
    ChunkWithProof(ChunkWithProof),
}

/// Error returned when constructing an instance of [`ValueOrChunk`].
#[derive(Debug, Error)]
pub enum ChunkingError {
    /// Merkle proof construction error.
    #[error("error constructing Merkle proof for chunk")]
    MerkleConstruction(
        #[from]
        #[source]
        MerkleConstructionError,
    ),
    /// Serialization error.
    #[error("error serializing data into chunks: {0}")]
    SerializationError(String),
}

impl<V> ValueOrChunk<V> {
    /// Creates an instance of [`ValueOrChunk::Value`] if data size is less than or equal to
    /// [`ChunkWithProof::CHUNK_SIZE_BYTES`] or a [`ValueOrChunk::ChunkWithProof`] if it is greater.
    /// In the latter case it will return only the `chunk_index`-th chunk of the value's byte
    /// representation.
    ///
    /// NOTE: The [`Chunkable`] instance used here needs to match the one used when calling
    /// [`Digest::hash_into_chunks_if_necessary`]. This is to ensure that type is turned into
    /// bytes consistently before chunking and hashing. If not then the Merkle proofs for chunks
    /// won't match.
    pub fn new(data: V, chunk_index: u64) -> Result<Self, ChunkingError>
    where
        V: Chunkable,
    {
        let bytes = Chunkable::as_bytes(&data).map_err(|error| {
            ChunkingError::SerializationError(format!(
                "failed to chunk {:?}: {:?}",
                std::any::type_name::<V>(),
                error
            ))
        })?;
        // NOTE: Cannot accept the chunk size bytes as an argument without changing the
        // IndexedMerkleProof. The chunk size there is hardcoded and will be used when
        // determining the chunk.
        if bytes.len() <= ChunkWithProof::CHUNK_SIZE_BYTES {
            Ok(ValueOrChunk::Value(data))
        } else {
            let chunk_with_proof = ChunkWithProof::new(&bytes, chunk_index)
                .map_err(ChunkingError::MerkleConstruction)?;
            Ok(ValueOrChunk::ChunkWithProof(chunk_with_proof))
        }
    }
}

impl Display for ValueOrChunk<TrieRaw> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ValueOrChunk::Value(data) => write!(f, "value {:10}", HexFmt(data.inner())),
            ValueOrChunk::ChunkWithProof(chunk) => write!(
                f,
                "chunk #{} with proof, root hash {}",
                chunk.proof().index(),
                chunk.proof().root_hash()
            ),
        }
    }
}

impl Display for ValueOrChunk<Vec<ExecutionResult>> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ValueOrChunk::Value(data) => write!(f, "value: {} execution results", data.len()),
            ValueOrChunk::ChunkWithProof(chunk) => write!(
                f,
                "chunk #{} with proof, root hash {}",
                chunk.proof().index(),
                chunk.proof().root_hash()
            ),
        }
    }
}

/// Error type simply conveying that chunk validation failed.
#[derive(Debug, Error)]
#[error("Chunk validation failed")]
pub(crate) struct ChunkValidationError;

/// Represents an enum that can contain either a whole trie or a chunk of it.
pub type TrieOrChunk = ValueOrChunk<TrieRaw>;

impl Item for TrieOrChunk {
    type Id = TrieOrChunkId;
    const TAG: Tag = Tag::TrieOrChunk;

    fn id(&self) -> Self::Id {
        match self {
            TrieOrChunk::Value(trie_raw) => TrieOrChunkId(0, Digest::hash(&trie_raw.inner())),
            TrieOrChunk::ChunkWithProof(chunked_data) => TrieOrChunkId(
                chunked_data.proof().index(),
                chunked_data.proof().root_hash(),
            ),
        }
    }
}

impl FetcherItem for TrieOrChunk {
    type ValidationError = ChunkWithProofVerificationError;
    type ValidationMetadata = EmptyValidationMetadata;

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        match self {
            TrieOrChunk::Value(_) => Ok(()),
            TrieOrChunk::ChunkWithProof(chunk_with_proof) => chunk_with_proof.verify(),
        }
    }
}

/// Represents the ID of a `TrieOrChunk` - containing the index and the root hash.
/// The root hash is the hash of the trie node as a whole.
/// The index is the index of a chunk if the node's size is too large and requires chunking. For
/// small nodes, it's always 0.
#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TrieOrChunkId(pub u64, pub Digest);

impl TrieOrChunkId {
    /// Returns the trie key part of the ID.
    pub fn digest(&self) -> &Digest {
        &self.1
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
        write!(f, "({}, {})", self.0, self.1)
    }
}

#[cfg(test)]
mod tests {
    use casper_hashing::ChunkWithProof;
    use casper_types::bytesrepr::Bytes;

    use super::ValueOrChunk;

    #[test]
    fn returns_value_or_chunk() {
        let input: Bytes = vec![1u8; 1].into();
        let value = ValueOrChunk::new(input, 0).unwrap();
        assert!(matches!(value, ValueOrChunk::Value { .. }));

        let input: Bytes = vec![1u8; ChunkWithProof::CHUNK_SIZE_BYTES + 1].into();
        let value_or_chunk = ValueOrChunk::new(input.clone(), 0).unwrap();
        let first_chunk = match value_or_chunk {
            ValueOrChunk::Value(_) => panic!("expected chunk"),
            ValueOrChunk::ChunkWithProof(chunk) => chunk,
        };

        // try to read all the chunks
        let chunk_count = first_chunk.proof().count();
        let mut chunks = vec![first_chunk];

        for i in 1..chunk_count {
            match ValueOrChunk::new(input.clone(), i).unwrap() {
                ValueOrChunk::Value(_) => panic!("expected chunk"),
                ValueOrChunk::ChunkWithProof(chunk) => chunks.push(chunk),
            }
        }

        // there should be no chunk with index `chunk_count`
        assert!(matches!(
            ValueOrChunk::new(input.clone(), chunk_count),
            Err(super::ChunkingError::MerkleConstruction(_))
        ));

        // all chunks should be valid
        assert!(chunks.iter().all(|chunk| chunk.verify().is_ok()));

        // reassemble the data
        let data: Vec<u8> = chunks
            .into_iter()
            .flat_map(|chunk| chunk.into_chunk())
            .collect();

        // Since `Bytes` are chunked "as-is", there's no deserialization of the bytes required.
        let retrieved_bytes: Bytes = data.into();

        assert_eq!(input, retrieved_bytes);
    }
}
