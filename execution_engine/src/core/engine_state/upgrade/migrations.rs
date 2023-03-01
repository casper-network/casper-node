use std::{
    cell::RefCell,
    cmp::Ordering,
    fmt,
    iter::FromIterator,
    ops::{Add, Div, Shr, Sub},
    rc::Rc,
    time::Instant,
};

use casper_hashing::Digest;
use casper_types::{EraId, Key, KeyTag, StoredValue};
use num::One;
use thiserror::Error;

use crate::{
    core::{execution, tracking_copy::TrackingCopy},
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie_store::operations::DeleteResult,
    },
};

#[derive(Debug)]
struct BinarySearchResult<T> {
    steps: usize,
    low: T,
    high: T,
}

fn bisect<
    T: Copy
        + fmt::Debug
        + PartialEq
        + PartialOrd
        + Sub<Output = T>
        + Add<Output = T>
        + Div<Output = T>
        + Shr<usize, Output = T>
        + One,
    E,
>(
    mut low: T,
    mut high: T,
    mut lookup: impl FnMut(T) -> Result<Ordering, E>,
) -> Result<BinarySearchResult<T>, E> {
    let mut steps = 0;
    while low <= high {
        dbg!(low, high);
        steps += 1;
        let mid = ((high - low) >> 1usize) + low;
        let mid_index = mid;

        match lookup(mid_index) {
            Ok(Ordering::Less) => {
                low = mid + T::one();
            }
            Ok(Ordering::Equal) => {
                break;
            }
            Ok(Ordering::Greater) => {
                high = mid - T::one();
            }
            Err(error) => return Err(error),
        }
    }

    Ok(BinarySearchResult { steps, low, high })
}

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
pub fn purge_era_info<S>(
    state: &S,
    mut state_root_hash: Digest,
    // largest_era_id: u64,
) -> Result<MigrationResult, Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error> + fmt::Debug,
{
    let correlation_id = CorrelationId::new();

    let mut tracking_copy = match state
        .checkout(state_root_hash)
        .map_err(|error| Error::Exec(error.into()))?
    {
        Some(tracking_copy) => Rc::new(RefCell::new(TrackingCopy::new(tracking_coxpy))),
        None => return Err(Error::RootNotFound),
    };

    let start = Instant::now();
    println!("Looking for largest era...");

    const FIRST_ERA: u64 = 0;
    const LARGEST_ERA: u64 = 10000;

    let tc = Rc::clone(&tracking_copy);

    let result = bisect(FIRST_ERA, LARGEST_ERA, move |idx| {
        match tc
            .borrow_mut()
            .get(correlation_id, &Key::EraInfo(EraId::new(idx)))
        {
            Ok(Some(_)) => Ok(Ordering::Less),
            Ok(None) => Ok(Ordering::Greater),
            Err(error) => Err(error),
        }
    })
    .map_err(|error| Error::Exec(error.into()))?;

    println!(
        "Found largest era {} with {} queries in {:?}",
        result.high,
        result.steps,
        start.elapsed()
    );

    let keys_to_delete: Vec<Key> = (0..result.high)
        .map(|era_id| Key::EraInfo(EraId::new(era_id)))
        .collect();

    if keys_to_delete.is_empty() {
        // Don't do any work if not keys are present in the global state.
        return Ok(MigrationResult {
            keys_to_delete: Vec::new(),
            era_summary: None,
            post_state_hash: state_root_hash,
        });
    }

    let last_era_info = match keys_to_delete.last() {
        Some(last_era_info) => tracking_copy
            .borrow_mut()
            .get(correlation_id, last_era_info)
            .map_err(|error| Error::UnableToRetrieveLastEraInfo(error.into()))?,
        None => None,
    };

    println!("Deleting {} keys...", keys_to_delete.len());

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

    println!(
        "Deleted {} keys in {:?}",
        keys_to_delete.len(),
        start.elapsed()
    );

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

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    #[test]
    fn should_run_bisect_on_partially_filled_map() {
        const LARGEST_ERA_ID: usize = 6059;
        const LOWEST_ERA_ID: usize = 0;
        const HIGHEST_ERA_ID: usize = u64::MAX as usize;

        let mut eras = Vec::new();
        for a in 0..=LARGEST_ERA_ID {
            eras.push(Some(a));
        }

        assert!(eras.get(LARGEST_ERA_ID).is_some());

        // assert_eq!(eras.len(), HIGHEST_ERA_ID);

        let result = super::bisect(LOWEST_ERA_ID, HIGHEST_ERA_ID, |idx| {
            match eras.get(idx) {
                Some(Some(_val)) => {
                    // Found something, means that idx is too low.
                    Ok(Ordering::Less)
                }
                Some(None) | None => {
                    if idx == 0 {
                        Err(())
                    } else {
                        // No value found, means that idx is too high.
                        Ok(Ordering::Greater)
                    }
                }
            }
        });
        dbg!(&result);

        assert_eq!(result.unwrap().high, LARGEST_ERA_ID);
    }

    #[test]
    fn should_run_bisect_on_empty_map() {
        const HIGHEST: usize = 1000000;

        let eras: Vec<Option<u64>> = vec![None; HIGHEST];

        let result = super::bisect(0, u64::MAX, |idx| {
            match eras.get(idx as usize) {
                Some(Some(val)) => {
                    // Found something, means that idx is too low.
                    Ok(Ordering::Less)
                }
                Some(None) => {
                    if idx == 0 {
                        return Err(());
                    }
                    // No value found, means that idx is too high.
                    Ok(Ordering::Greater)
                }
                None => Ok(Ordering::Greater),
            }
        });

        assert!(result.is_err());

        // assert_eq!(result.unwrap().high, 1);
    }
}
