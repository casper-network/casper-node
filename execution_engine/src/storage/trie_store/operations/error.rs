use thiserror::Error;

use crate::shared::newtypes::Blake2bHash;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CorruptDatabaseError {
    #[error(
        "Key {trie_key} is not the hash of value {trie_value} (should be {hash_of_trie_value})"
    )]
    KeyIsNotHashOfValue {
        trie_key: Blake2bHash,
        trie_value: String,
        hash_of_trie_value: Blake2bHash,
    },
}
