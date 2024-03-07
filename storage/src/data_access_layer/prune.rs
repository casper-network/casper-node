//! Support for pruning leaf nodes from the merkle trie.
use crate::{
    global_state::trie_store::operations::TriePruneResult, tracking_copy::TrackingCopyError,
};
use casper_types::{execution::Effects, Digest, Key};

/// Represents the configuration of a prune operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneRequest {
    state_hash: Digest,
    keys_to_prune: Vec<Key>,
}

impl PruneRequest {
    /// Create new prune config.
    pub fn new(state_hash: Digest, keys_to_prune: Vec<Key>) -> Self {
        PruneRequest {
            state_hash,
            keys_to_prune,
        }
    }

    /// Returns the current state root state hash
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns the list of keys to delete.
    pub fn keys_to_prune(&self) -> &[Key] {
        &self.keys_to_prune
    }
}

/// The result of performing a prune.
#[derive(Debug, Clone)]
pub enum PruneResult {
    /// Root not found.
    RootNotFound,
    /// Key does not exists.
    MissingKey,
    /// Failed to prune.
    Failure(TrackingCopyError),
    /// New state root hash generated after elements were pruned.
    Success {
        /// State root hash.
        post_state_hash: Digest,
        /// Effects of executing a step request.
        effects: Effects,
    },
}

impl From<TriePruneResult> for PruneResult {
    fn from(value: TriePruneResult) -> Self {
        match value {
            TriePruneResult::Pruned(post_state_hash) => PruneResult::Success {
                post_state_hash,
                effects: Effects::default(),
            },
            TriePruneResult::MissingKey => PruneResult::MissingKey,
            TriePruneResult::RootNotFound => PruneResult::RootNotFound,
            TriePruneResult::Failure(gse) => PruneResult::Failure(TrackingCopyError::Storage(gse)),
        }
    }
}
