//! A store for persisting `Trie` values at their hashes.
//!
//! See the [lmdb](lmdb/index.html#usage) modules for usage examples.
pub mod lmdb;
/// Trie store operational logic.
pub mod operations;

// An in-mem cache backed up by a store that is used to optimize batch writes.
mod cache;

pub(crate) use cache::CacheError as TrieStoreCacheError;

#[cfg(test)]
mod tests;

use casper_types::Digest;

use crate::global_state::{store::Store, trie::Trie};

const NAME: &str = "TRIE_STORE";

/// An entity which persists [`Trie`] values at their hashes.
pub trait TrieStore<K, V>: Store<Digest, Trie<K, V>> {}
