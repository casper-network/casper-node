use std::{cell::RefCell, iter::FromIterator, rc::Rc};

use casper_hashing::Digest;
use casper_types::{Key, KeyTag, StoredValue};
use thiserror::Error;

use crate::{
    core::{execution, tracking_copy::TrackingCopy},
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie_store::operations::DeleteResult,
    },
};

#[derive(Clone, Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("exec error: {0}")]
    Exec(execution::Error),
    #[error("unable to retreive era info keys")]
    UnableToRetriveEraInfoKeys(execution::Error),
    #[error("unable to delete era info key")]
    UnableToDeleteEraInfoKeys(execution::Error),
    #[error("unable to retrieve last era info")]
    UnableToRetrieveLastEraInfo(execution::Error),
    #[error("root not found")]
    RootNotFound,
    #[error("key does not exists")]
    KeyDoesNotExists,
}

pub struct MigrationResult {
    pub keys_to_delete: Vec<Key>,
    pub era_summary: Option<StoredValue>,
    pub post_state_hash: Digest,
}

/// Purges [`Key::EraInfo`] keys from the tip of the store and writes only single key with the
/// latest era into a stable key [`Key::EraSummary`].
pub fn purge_era_info<S>(state: &S, mut state_root_hash: Digest) -> Result<MigrationResult, Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    let correlation_id = CorrelationId::new();

    let tracking_copy = match state
        .checkout(state_root_hash)
        .map_err(|error| Error::Exec(error.into()))?
    {
        Some(tracking_copy) => {
            let tc = TrackingCopy::new(tracking_copy);
            Rc::new(RefCell::new(tc))
        }
        None => return Err(Error::RootNotFound),
    };

    let keys = tracking_copy
        .borrow_mut()
        .get_keys(correlation_id, &KeyTag::EraInfo)
        .map_err(|error| Error::UnableToRetriveEraInfoKeys(error.into()))?;

    if keys.is_empty() {
        // Don't do any work if not keys are present in the global state.
        return Ok(MigrationResult {
            keys_to_delete: Vec::new(),
            era_summary: None,
            post_state_hash: state_root_hash,
        });
    }

    let keys_to_delete = Vec::from_iter(keys);

    let last_era_info = match keys_to_delete.last() {
        Some(last_era_info) => tracking_copy
            .borrow_mut()
            .get(correlation_id, last_era_info)
            .map_err(|error| Error::UnableToRetrieveLastEraInfo(error.into()))?,
        None => None,
    };

    if !keys_to_delete.is_empty() {
        match state
            .delete_keys(correlation_id, state_root_hash, &keys_to_delete)
            .map_err(|error| Error::UnableToDeleteEraInfoKeys(error.into()))?
        {
            DeleteResult::Deleted(new_post_state_hash) => {
                state_root_hash = new_post_state_hash;
            }
            DeleteResult::DoesNotExist => return Err(Error::KeyDoesNotExists),
            DeleteResult::RootNotFound => return Err(Error::RootNotFound),
        }
    }

    if let Some(last_era_info) = last_era_info.as_ref() {
        let mut tracking_copy = match state
            .checkout(state_root_hash)
            .map_err(|error| Error::Exec(error.into()))?
        {
            Some(tracking_copy) => TrackingCopy::new(tracking_copy),
            None => return Err(Error::RootNotFound),
        };

        tracking_copy.force_write(Key::EraSummary, last_era_info.clone());

        let new_state_root_hash = state
            .commit(
                correlation_id,
                state_root_hash,
                tracking_copy.effect().transforms,
            )
            .map_err(|error| Error::Exec(error.into()))?;

        state_root_hash = new_state_root_hash;
    }

    Ok(MigrationResult {
        keys_to_delete,
        era_summary: last_era_info,
        post_state_hash: state_root_hash,
    })
}
