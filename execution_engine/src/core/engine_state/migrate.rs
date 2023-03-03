//! Module containing migration-specific code.

pub mod purge_era_info;

use casper_hashing::Digest;
use casper_types::EraId;

use self::purge_era_info::PurgedEraInfo;

/// Represents an action taken in a migration.
pub enum MigrateAction {
    /// Purge era info objects from the trie.
    PurgeEraInfo {
        /// How many deletes per migration.
        batch_size: usize,

        /// The current era at the time of the migration.
        current_era_id: u64,
    },
    /// Migrate Key::EraInfo(id) -> Key::EraSummary. Should happen once.
    WriteStableEraInfo {
        /// the era id to use for this migration.
        era_id: EraId,
    },
}

impl MigrateAction {
    /// Purge era info objects from the trie.
    pub fn purge_era_info(batch_size: usize, current_era_id: u64) -> Self {
        Self::PurgeEraInfo {
            batch_size,
            current_era_id,
        }
    }
    /// Migrate Key::EraInfo(id) -> Key::EraSummary. Should happen once.
    pub fn write_stable_era_info(era_id: EraId) -> Self {
        Self::WriteStableEraInfo { era_id }
    }
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

/// A successful migration.
#[derive(Debug, Clone)]
pub struct MigrateSuccess {
    /// Post state hash of completed migration.
    pub post_state_hash: Digest,
    /// Actions takes in the migration.
    pub actions_taken: Vec<ActionSuccess>,
}

impl MigrateSuccess {
    /// Create a new MigrateSuccess.
    pub fn new(post_state_hash: Digest, actions_taken: Vec<ActionSuccess>) -> Self {
        Self {
            post_state_hash,
            actions_taken,
        }
    }
}

/// Successful action.
#[derive(Debug, Clone)]
pub enum ActionSuccess {
    /// Action for purging era info objects.
    PurgeEraInfo(PurgedEraInfo),
    /// Action for writing stable key for era summary, used once from EraInfo(id) -> EraSummary.
    WroteStableKey {
        /// Post state hash.
        post_state_hash: Digest,
    },
}

impl ActionSuccess {
    /// Post state hash from success.
    pub fn post_state_hash(&self) -> Digest {
        match self {
            ActionSuccess::PurgeEraInfo(PurgedEraInfo {
                post_state_hash, ..
            }) => *post_state_hash,
            ActionSuccess::WroteStableKey { post_state_hash } => *post_state_hash,
        }
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
    WriteStableKey(#[from] stable_era_info::StableKeyError),
}

/// Implements a migration for copying the current era info to a stable key.
pub mod stable_era_info {
    use super::*;
    use std::borrow::BorrowMut;

    use crate::{
        core::{execution, tracking_copy::TrackingCopy},
        shared::newtypes::CorrelationId,
        storage::global_state::{CommitProvider, StateProvider},
    };
    use casper_types::{EraId, Key};

    /// Errors that can occur while purging era info objects from global state.
    #[derive(Clone, thiserror::Error, Debug)]
    #[non_exhaustive]
    pub enum StableKeyError {
        /// Execution Engine error.
        #[error("exec error: {0}")]
        Exec(execution::Error),

        /// Unable to retrieve last era info.
        #[error("unable to retrieve last era info")]
        UnableToRetrieveLastEraInfo(execution::Error),
        /// Root not found.
        #[error("root not found")]
        RootNotFound,

        /// Key does not exist.
        #[error("key does not exist")]
        KeyDoesNotExist,
    }

    /// Write era info currently at era_id(number) key to stable key.
    pub fn write_era_info_summary_to_stable_key<S>(
        state: &S,
        correlation_id: CorrelationId,
        state_root_hash: Digest,
        era_id: EraId,
    ) -> Result<ActionSuccess, StableKeyError>
    where
        S: StateProvider + CommitProvider,
        S::Error: Into<execution::Error>,
    {
        let mut tracking_copy = match state
            .checkout(state_root_hash)
            .map_err(|error| StableKeyError::Exec(error.into()))?
        {
            Some(tracking_copy) => TrackingCopy::new(tracking_copy),
            None => return Err(StableKeyError::RootNotFound),
        };

        let last_era_info = tracking_copy
            .borrow_mut()
            .get(correlation_id, &Key::EraInfo(era_id))
            .map_err(|error| StableKeyError::UnableToRetrieveLastEraInfo(error.into()))?
            .ok_or(StableKeyError::KeyDoesNotExist)?;

        tracking_copy.force_write(Key::EraSummary, last_era_info);

        let new_state_root_hash = state
            .commit(
                correlation_id,
                state_root_hash,
                tracking_copy.effect().transforms,
            )
            .map_err(|error| StableKeyError::Exec(error.into()))?;

        Ok(ActionSuccess::WroteStableKey {
            post_state_hash: new_state_root_hash,
        })
    }
}
