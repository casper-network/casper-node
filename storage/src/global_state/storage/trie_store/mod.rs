//! A store for persisting `Trie` values at their hashes.
//!
//! See the [lmdb](lmdb/index.html#usage) modules for usage examples.
pub mod lmdb;
pub mod operations;
#[cfg(test)]
mod tests;

use casper_types::Digest;

use crate::global_state::storage::{store::Store, trie::Trie};

const NAME: &str = "TRIE_STORE";

/// An entity which persists [`Trie`] values at their hashes.
pub trait TrieStore<K, V>: Store<Digest, Trie<K, V>> {}
