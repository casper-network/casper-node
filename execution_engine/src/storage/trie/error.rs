use casper_hashing::MerkleConstructionError;
use casper_types::bytesrepr;

use thiserror::Error;

/// Errors that can occur when calculating the Trie hash.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TrieHashingError {
    /// Serialization error.
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),
    /// Merkle proof construction error.
    #[error("{0}")]
    MerkleConstruction(MerkleConstructionError),
}

impl From<bytesrepr::Error> for TrieHashingError {
    fn from(error: bytesrepr::Error) -> Self {
        TrieHashingError::BytesRepr(error)
    }
}

impl From<MerkleConstructionError> for TrieHashingError {
    fn from(error: MerkleConstructionError) -> Self {
        TrieHashingError::MerkleConstruction(error)
    }
}
