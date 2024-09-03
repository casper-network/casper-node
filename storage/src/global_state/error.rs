use std::sync;

use thiserror::Error;

use casper_types::{bytesrepr, Digest, Key};

use crate::global_state::{state::CommitError, trie::TrieRaw};

use super::trie_store::TrieStoreCacheError;

/// Error enum representing possible errors in global state interactions.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// LMDB error returned from underlying `lmdb` crate.
    #[error(transparent)]
    Lmdb(#[from] lmdb::Error),

    /// (De)serialization error.
    #[error("{0}")]
    BytesRepr(#[from] bytesrepr::Error),

    /// Concurrency error.
    #[error("Another thread panicked while holding a lock")]
    Poison,

    /// Error committing to execution engine.
    #[error(transparent)]
    Commit(#[from] CommitError),

    /// Invalid state root hash.
    #[error("RootNotFound")]
    RootNotFound,

    /// Failed to put a trie node into global state because some of its children were missing.
    #[error("Failed to put a trie into global state because some of its children were missing")]
    MissingTrieNodeChildren(Digest, TrieRaw, Vec<Digest>),

    /// Failed to prune listed keys.
    #[error("Pruning attempt failed.")]
    FailedToPrune(Vec<Key>),

    /// Cannot provide proofs over working state in a cache (programmer error).
    #[error("Attempt to generate proofs using non-empty cache.")]
    CannotProvideProofsOverCachedData,

    /// Encountered a cache error.
    #[error("Cache error")]
    CacheError(#[from] TrieStoreCacheError),
}

impl<T> From<sync::PoisonError<T>> for Error {
    fn from(_error: sync::PoisonError<T>) -> Self {
        Error::Poison
    }
}
