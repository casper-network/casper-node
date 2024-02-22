use std::sync;

use thiserror::Error;

use casper_types::{bytesrepr, Digest, Key};

use crate::global_state::{state::CommitError, trie::TrieRaw};

/// Error enum representing possible errors in global state interactions.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// LMDB error returned from underlying `lmdb` crate.
    #[error(transparent)]
    Lmdb(#[from] lmdb::Error),

    /// (De)serialization error.
    #[error("{0}")]
    BytesRepr(bytesrepr::Error),

    /// Concurrency error.
    #[error("Another thread panicked while holding a lock")]
    Poison,

    /// Error committing to execution engine.
    #[error(transparent)]
    CommitError(#[from] CommitError),

    /// Invalid state root hash.
    #[error("RootNotFound")]
    RootNotFound,

    /// Failed to put a trie node into global state because some of its children were missing.
    #[error("Failed to put a trie into global state because some of its children were missing")]
    MissingTrieNodeChildren(Digest, TrieRaw, Vec<Digest>),

    /// Failed to prune listed keys.
    #[error("Pruning attempt failed.")]
    FailedToPrune(Vec<Key>),
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::BytesRepr(error)
    }
}

impl<T> From<sync::PoisonError<T>> for Error {
    fn from(_error: sync::PoisonError<T>) -> Self {
        Error::Poison
    }
}
