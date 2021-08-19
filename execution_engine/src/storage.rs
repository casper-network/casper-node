/// Storage errors.
pub mod error;
/// Global State.
pub mod global_state;
/// Store module.
pub mod store;
/// Transaction Source.
pub mod transaction_source;
/// Merkle Trie implementation.
pub mod trie;
/// Merkle Trie storage.
pub mod trie_store;

const MAX_DBS: u32 = 2;

#[cfg(test)]
pub(crate) const DEFAULT_TEST_MAX_DB_SIZE: usize = 52_428_800; // 50 MiB

#[cfg(test)]
pub(crate) const DEFAULT_TEST_MAX_READERS: u32 = 512;
