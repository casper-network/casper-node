//! Module containing the purge-era-info migration.

use std::{cell::RefCell, rc::Rc};

use casper_hashing::Digest;
use casper_types::{EraId, Key};
use thiserror::Error;

use crate::{
    core::{execution, tracking_copy::TrackingCopy},
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie_store::operations::DeleteResult,
    },
};

/// Errors that can occur while purging era info objects from global state.
#[derive(Clone, Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Execution Engine error.
    #[error("exec error: {0}")]
    Exec(execution::Error),

    /// Unable to retrieve key.
    #[error("unable to retreive era info keys")]
    UnableToRetriveEraInfoKeys(execution::Error),

    /// Unable to delete era info keys.
    #[error("unable to delete era info key")]
    UnableToDeleteEraInfoKeys(execution::Error),

    /// Root not found.
    #[error("root not found")]
    RootNotFound,

    /// Key does not exist.
    #[error("key does not exist")]
    KeyDoesNotExist,
}

/// Result of purging eras migration.
#[derive(Debug, Clone)]
pub struct PurgedEraInfo {
    /// Resulting state root hash after completed migration.
    pub post_state_hash: Digest,
    /// Keys that were deleted.
    pub keys_deleted: Vec<Key>,
}

const LOWER_BOUND_ERA: u64 = 0;

/// Successful action.
#[derive(Debug, Clone)]
pub enum PurgeEraInfoSuccess {
    /// Era info purged,
    Progress(PurgedEraInfo),
    /// Complete, no changes were made to global state.
    Complete,
}

/// Purges exactly `batch_size` of [`Key::EraInfo`] keys from the tip of the store.
/// Must be called until `PurgeEraInfoSuccess::Complete` is returned.
pub fn purge_era_info<S>(
    state: &S,
    mut state_root_hash: Digest,
    upper_bound_era_id: u64,
    batch_size: u32,
) -> Result<PurgeEraInfoSuccess, Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    let correlation_id = CorrelationId::new();
    let tracking_copy = match state
        .checkout(state_root_hash)
        .map_err(|error| Error::Exec(error.into()))?
    {
        Some(tracking_copy) => Rc::new(RefCell::new(TrackingCopy::new(tracking_copy))),
        None => return Err(Error::RootNotFound),
    };

    let tc1 = Rc::clone(&tracking_copy);
    let mut lower_bound_era_id = LOWER_BOUND_ERA;
    let mut checked_for_era_info_up_to: u64 = 0;

    // In the worst case, we will ask for every era info key.
    for idx in LOWER_BOUND_ERA..upper_bound_era_id {
        if tc1
            .borrow_mut()
            .get(correlation_id, &Key::EraInfo(EraId::new(idx)))
            .map_err(|err| Error::Exec(err.into()))?
            .is_some()
        {
            lower_bound_era_id = idx;
            break;
        }
        checked_for_era_info_up_to += 1;
    }
    if checked_for_era_info_up_to == upper_bound_era_id {
        // Don't do any work if the range of eras is empty.
        return Ok(PurgeEraInfoSuccess::Complete);
    }

    let max_bound = (lower_bound_era_id + batch_size as u64).min(upper_bound_era_id + 1);
    let keys_to_delete: Vec<Key> = (lower_bound_era_id..max_bound)
        .map(|era_id| Key::EraInfo(EraId::new(era_id)))
        .collect();

    match state
        .delete_keys(correlation_id, state_root_hash, &keys_to_delete)
        .map_err(|error| Error::UnableToDeleteEraInfoKeys(error.into()))?
    {
        DeleteResult::Deleted(new_post_state_hash) => {
            state_root_hash = new_post_state_hash;
        }
        DeleteResult::DoesNotExist => return Err(Error::KeyDoesNotExist),
        DeleteResult::RootNotFound => return Err(Error::RootNotFound),
    }
    Ok(PurgeEraInfoSuccess::Progress(PurgedEraInfo {
        post_state_hash: state_root_hash,
        keys_deleted: keys_to_delete,
    }))
}
