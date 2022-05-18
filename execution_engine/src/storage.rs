//! Storage for the execution engine.

/// Storage errors.
pub mod error;
/// Global State.
pub mod global_state;
/// Store module.
pub mod store;
/// Merkle Trie implementation.
pub mod trie;
/// Merkle Trie storage.
pub mod trie_store;

/// Generic data store.
pub mod db_store;

const MAX_DBS: u32 = 2;

pub use db_store::ROCKS_DB_DATA_DIR;
