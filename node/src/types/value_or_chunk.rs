use std::fmt::{self, Debug, Display, Formatter};

use casper_execution_engine::storage::trie::TrieRaw;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_hashing::{ChunkWithProof, Digest, MerkleConstructionError};
use casper_types::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use datasize::DataSize;

/// Represents a value or a chunk of data with attached proof.
/// Chunk with attached proof is used when the requested
/// value is larger than [ChunkWithProof::CHUNK_SIZE_BYTES].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ValueOrChunk<V> {
    /// Represents a value.
    Value(V),
    /// Represents a chunk of data with attached proof.
    ChunkWithProof(ChunkWithProof),
}

#[derive(Debug, PartialEq, Error, Eq)]
/// Error returned when constructing an instance of `ValueOrChunk`.
pub enum ChunkingError {
    /// Merkle proof construction error
    MerkleConstruction(MerkleConstructionError),
    /// Serialization error
    SerializationError(bytesrepr::Error),
}

impl Display for ChunkingError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ChunkingError::MerkleConstruction(error) => f
                .debug_tuple("MerkleConstructionError")
                .field(&error)
                .finish(),
            ChunkingError::SerializationError(error) => {
                f.debug_tuple("SerializationError").field(&error).finish()
            }
        }
    }
}

impl<V> ValueOrChunk<V> {
    /// Creates an instance of `ValueOrChunk::Value` if data is less than `max_chunk_size`
    /// or a `ValueOrChunk::ChunkWithProof` if it is. In the latter case it will return only the
    /// `chunk_index`-th chunk of the value's byte representation.
    pub fn new(data: V, chunk_index: u64, max_chunk_size: usize) -> Result<Self, ChunkingError>
    where
        V: ToBytes,
    {
        let bytes = ToBytes::to_bytes(&data).map_err(ChunkingError::SerializationError)?;
        if bytes.len() <= max_chunk_size {
            Ok(ValueOrChunk::Value(data))
        } else {
            let chunk_with_proof = ChunkWithProof::new(&bytes, chunk_index)
                .map_err(ChunkingError::MerkleConstruction)?;
            Ok(ValueOrChunk::ChunkWithProof(chunk_with_proof))
        }
    }
}

/// Represents an enum that can contain either a whole trie or a chunk of it.
pub type TrieOrChunk = ValueOrChunk<TrieRaw>;

impl<V: Debug> Display for ValueOrChunk<V> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ValueOrChunk::Value(data) => f.debug_tuple("Value").field(data).finish(),
            ValueOrChunk::ChunkWithProof(chunk) => f
                .debug_struct("ChunkWithProof")
                .field("index", &chunk.proof().index())
                .field("hash", &chunk.proof().root_hash())
                .finish(),
        }
    }
}

impl<V> ValueOrChunk<V> {
    const VALUE_TAG: u8 = 0;
    const CHUNK_TAG: u8 = 1;

    fn tag(&self) -> u8 {
        match self {
            ValueOrChunk::Value(_) => Self::VALUE_TAG,
            ValueOrChunk::ChunkWithProof(_) => Self::CHUNK_TAG,
        }
    }
}

impl<V: ToBytes> ToBytes for ValueOrChunk<V> {
    fn write_bytes(&self, buf: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        buf.push(self.tag());

        match self {
            ValueOrChunk::Value(data) => {
                data.write_bytes(buf)?;
            }
            ValueOrChunk::ChunkWithProof(chunk) => {
                chunk.write_bytes(buf)?;
            }
        }

        Ok(())
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut ret)?;
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ValueOrChunk::Value(data) => data.serialized_length(),
                ValueOrChunk::ChunkWithProof(chunk) => chunk.serialized_length(),
            }
    }
}

impl<V: FromBytes> FromBytes for ValueOrChunk<V> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            Self::VALUE_TAG => {
                let (value, rem) = V::from_bytes(rem)?;
                Ok((ValueOrChunk::Value(value), rem))
            }
            Self::CHUNK_TAG => {
                let (chunk, rem) = ChunkWithProof::from_bytes(rem)?;
                Ok((ValueOrChunk::ChunkWithProof(chunk), rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
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
pub(crate) struct TrieOrChunkIdDisplay<'a>(pub &'a [u8]);

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
mod value_chunking {
    use super::ValueOrChunk;

    #[test]
    fn returns_value_or_chunk() {
        let max_chunk_size = u32::MAX as usize;

        // Smaller than the maximum chunk size.
        let input = 1u32;

        let value = ValueOrChunk::new(input, 0, max_chunk_size).unwrap();
        assert!(matches!(value, ValueOrChunk::Value { .. }));
    }
}
