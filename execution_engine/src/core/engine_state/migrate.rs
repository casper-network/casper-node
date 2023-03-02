pub mod purge_era_info;

use thiserror::Error;

use casper_hashing::Digest;

/// Represents an action taken in a migration.
pub enum MigrateAction {
    /// Purge era info objects from the trie.
    PurgeEraInfo {
        /// How many deletes per migration.
        batch_size: usize,

        /// The current era at the time of the migration.
        current_era_id: u64,
    },
}

/// Represents a migration with one or more actions.
pub struct MigrateConfig {
    /// State root hash.
    pub state_root_hash: Digest,
    /// Actions taken during migration.
    pub actions: Vec<MigrateAction>,
}

impl MigrateConfig {
    /// Create a new MigrateConfig.
    pub fn new(state_root_hash: Digest, actions: Vec<MigrateAction>) -> Self {
        Self {
            state_root_hash,
            actions,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MigrateSuccess {
    pub post_state_hash: Digest,
}

#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum MigrateError {
    #[error(transparent)]
    PurgeEraInfo(#[from] purge_era_info::Error),
}
