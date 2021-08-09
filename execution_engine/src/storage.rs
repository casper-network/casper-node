#![allow(missing_docs)]

// modules
pub mod error;
pub mod global_state;
pub mod store;
pub mod transaction_source;
pub mod trie;
pub mod trie_store;

const MAX_DBS: u32 = 2;

#[cfg(test)]
pub(crate) const DEFAULT_TEST_MAX_DB_SIZE: usize = 52_428_800; // 50 MiB

#[cfg(test)]
pub(crate) const DEFAULT_TEST_MAX_READERS: u32 = 512;
