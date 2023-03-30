//! Support for purging leaf nodes from the merkle trie.
use casper_hashing::Digest;
use casper_types::Key;

/// Represents a successfully performed purge.
#[derive(Debug, Clone)]
pub enum PurgeResult {
    /// Root not found.
    RootNotFound,
    /// Key does not exists.
    DoesNotExist,
    /// New state root hash generated after elements were purged.
    Success {
        /// State root hash.
        post_state_hash: Digest,
    },
}

/// Represents the configuration of a protocol upgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeConfig {
    pre_state_hash: Digest,
    keys_to_delete: Vec<Key>,
}

impl PurgeConfig {
    /// Create new upgrade config.
    #[allow(clippy::too_many_arguments)]
    pub fn new(pre_state_hash: Digest, keys_to_purge: Vec<Key>) -> Self {
        PurgeConfig {
            pre_state_hash,
            keys_to_delete: keys_to_purge,
        }
    }

    /// Returns the current state root state hash
    pub fn pre_state_hash(&self) -> Digest {
        self.pre_state_hash
    }

    /// Returns the list of keys to delete.
    pub fn keys_to_delete(&self) -> &[Key] {
        &self.keys_to_delete
    }
}
