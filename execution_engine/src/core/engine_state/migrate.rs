//! Module containing migration-specific code.

pub mod purge_era_info;
pub mod write_stable_era_info;

use casper_hashing::Digest;
use casper_types::EraId;

/// Represents an action taken in a migration.
pub enum MigrateAction {
    /// Purge era info objects from the trie.
    PurgeEraInfo {
        /// How many deletes per migration.
        batch_size: u32,

        /// The current era at the time of the migration.
        current_era_id: EraId,
    },
    /// Migrate Key::EraInfo(id) -> Key::EraSummary. Should happen once.
    WriteStableEraInfo {
        /// the era id to use for this migration.
        era_id: EraId,
    },
}

impl MigrateAction {
    /// Purge era info objects from the trie.
    pub fn purge_era_info(batch_size: u32, current_era_id: impl Into<EraId>) -> Self {
        Self::PurgeEraInfo {
            batch_size,
            current_era_id: current_era_id.into(),
        }
    }
    /// Migrate Key::EraInfo(id) -> Key::EraSummary. Should happen once.
    pub fn write_stable_era_info(era_id: EraId) -> Self {
        Self::WriteStableEraInfo { era_id }
    }
}

/// Represents a migration with one or more actions.
pub struct MigrationActions {
    /// Pre state root hash.
    pub pre_state_root_hash: Digest,
    /// Actions taken during migration.
    pub actions: Vec<MigrateAction>,
}

impl MigrationActions {
    /// Create a new MigrateConfig.
    pub fn new(pre_state_root_hash: Digest, actions: Vec<MigrateAction>) -> Self {
        Self {
            pre_state_root_hash,
            actions,
        }
    }
}

/// A successful migration.
#[derive(Debug, Clone)]
pub struct MigrateSuccess {
    /// Post state hash of completed migration.
    pub post_state_hash: Digest,
}

impl MigrateSuccess {
    /// Create a new MigrateSuccess.
    pub fn new(post_state_hash: Digest) -> Self {
        Self { post_state_hash }
    }
}

/// An error occurred during a migration.
#[derive(Debug, thiserror::Error, Clone)]
#[non_exhaustive]
pub enum MigrateError {
    /// Error occurred during PurgeEraInfo migration.
    #[error(transparent)]
    PurgeEraInfo(#[from] purge_era_info::Error),
    /// Error occurred during WriteStableKey migration.
    #[error(transparent)]
    WriteStableKey(#[from] write_stable_era_info::StableKeyError),
}
