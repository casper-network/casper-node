/// Storage errors.
pub mod error;
/// Global State.
pub mod state;
/// Store module.
pub mod store;
/// Transaction Source.
pub mod transaction_source;
/// Merkle Trie implementation.
pub mod trie;
/// Merkle Trie storage.
pub mod trie_store;

const MAX_DBS: u32 = 2;

pub(crate) const DEFAULT_MAX_DB_SIZE: usize = 52_428_800; // 50 MiB

pub(crate) const DEFAULT_MAX_READERS: u32 = 512;

pub(crate) const DEFAULT_MAX_QUERY_DEPTH: u64 = 6;
