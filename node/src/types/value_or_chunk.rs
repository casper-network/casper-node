use std::fmt::{self, Debug, Display, Formatter};

use casper_execution_engine::storage::trie::TrieRaw;
use casper_types::ExecutionResult;
use datasize::DataSize;
use hex_fmt::HexFmt;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_hashing::{ChunkWithProof, Digest, MerkleConstructionError};

use super::Chunkable;
use crate::utils::ds;

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

impl Display for ValueOrChunk<HashingTrieRaw> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ValueOrChunk::Value(data) => write!(f, "value {}", data),
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, DataSize)]
pub struct HashingTrieRaw {
    inner: TrieRaw,
    #[serde(skip)]
    #[data_size(with = ds::once_cell)]
    hash: OnceCell<Digest>,
}

impl From<TrieRaw> for HashingTrieRaw {
    fn from(inner: TrieRaw) -> HashingTrieRaw {
        HashingTrieRaw {
            inner,
            hash: OnceCell::new(),
        }
    }
}

impl Display for HashingTrieRaw {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:10}", HexFmt(self.inner.inner()))
    }
}

impl HashingTrieRaw {
    pub(crate) fn hash(&self) -> Digest {
        *self.hash.get_or_init(|| Digest::hash(self.inner.inner()))
    }

    pub fn inner(&self) -> &TrieRaw {
        &self.inner
    }

    pub fn into_inner(self) -> TrieRaw {
        self.inner
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
