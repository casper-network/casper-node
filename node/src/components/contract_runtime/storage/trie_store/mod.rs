//! A store for persisting [`Trie`](crate::trie::Trie) values at their hashes.
//!
//! See the [in_memory](in_memory/index.html#usage) and
//! [lmdb](lmdb/index.html#usage) modules for usage examples.
pub mod in_memory;
pub mod lmdb;
pub(crate) mod operations;
#[cfg(test)]
mod tests;

use crate::components::contract_runtime::shared::newtypes::Blake2bHash;

use crate::components::contract_runtime::storage::{store::Store, trie::Trie};

const NAME: &str = "TRIE_STORE";

/// An entity which persists [`Trie`] values at their hashes.
pub trait TrieStore<K, V>: Store<Blake2bHash, Trie<K, V>> {}
