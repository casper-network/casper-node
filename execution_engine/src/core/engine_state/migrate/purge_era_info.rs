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
use num::{Num, One};
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

fn bisect<T: Copy + fmt::Debug + Num + PartialOrd + Shr<usize, Output = T>, E>(
    mut low: T,
    upper_bound: T,
    mut lookup: impl FnMut(T) -> Result<Ordering, E>,
) -> Result<BinarySearchResult<T>, E> {
    let mut high = upper_bound;
    let mut steps = 0;
    while low <= high {
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

/// Result of purging eras migration.
pub struct PurgeEraInfoResult {
    /// Keys that were deleted.
    pub keys_deleted: Vec<Key>,
    /// New era summary generated.
    pub era_summary: Option<StoredValue>,
    /// Post state hash.
    pub post_state_hash: Digest,
}

const ERAS_TO_DELETE_PER_STEP: usize = 5; // TODO: Chainspec
const LOWER_BOUND_ERA: u64 = 0;

/// Purges [`Key::EraInfo`] keys from the tip of the store and writes only single key with the
/// latest era into a stable key [`Key::EraSummary`].
pub fn purge_era_info<S>(
    state: &S,
    mut state_root_hash: Digest,
    upper_bound_era: u64,
    batch_size: usize,
) -> Result<PurgeEraInfoResult, Error>
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

    let start = Instant::now();

    let tc = Rc::clone(&tracking_copy);

    let result = bisect(LOWER_BOUND_ERA, upper_bound_era, move |idx| {
        match tc
            .borrow_mut()
            .get(correlation_id, &Key::EraInfo(EraId::new(idx)))
        {
            Ok(Some(era_id)) => {
                if idx == 0 {
                    // No eras were removed yet. This is the first migration.
                    Ok(Ordering::Equal)
                } else {
                    Ok(Ordering::Greater)
                }
            }
            Ok(None) => Ok(Ordering::Less),
            Err(error) => Err(error),
        }
    })
    .map_err(|error| Error::Exec(error.into()))?;
    dbg!(&result);
    println!(
        "Found smallest era {} with {} queries in {:?}",
        result.low,
        result.steps,
        start.elapsed()
    );

    // Determine state (i.e. start, or continue)

    if result.low == 0 {
        // TODO
        // Lower bound starts with era id == 0, which means this is the first migration step.
        // Now find highest era info in range, then copy it over to Key::EraSummary
        // todo!();
    }

    let max_bound = (result.low + batch_size as u64).min(upper_bound_era);
    let keys_to_delete: Vec<Key> = (result.low..max_bound)
        .map(|era_id| Key::EraInfo(EraId::new(era_id)))
        .collect();

    dbg!(&keys_to_delete);

    if keys_to_delete.is_empty() {
        // Don't do any work if not keys are present in the global state.
        return Ok(PurgeEraInfoResult {
            keys_deleted: Vec::new(),
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
        DeleteResult::DoesNotExist => return Err(Error::KeyDoesNotExist),
        DeleteResult::RootNotFound => return Err(Error::RootNotFound),
    }

    println!(
        "Deleted {} keys in {:?}",
        keys_to_delete.len(),
        start.elapsed()
    );

    if let Some(last_era_info) = last_era_info.as_ref() {
        write_era_info_summary_to_stable_key(
            state,
            state_root_hash,
            last_era_info,
            correlation_id,
        )?;
    }

    Ok(PurgeEraInfoResult {
        keys_deleted: keys_to_delete,
        era_summary: last_era_info,
        post_state_hash: state_root_hash,
    })
}

fn write_era_info_summary_to_stable_key<S>(
    state: &S,
    state_root_hash: Digest,
    last_era_info: &StoredValue,
    correlation_id: CorrelationId,
) -> Result<Digest, Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
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
    Ok(new_state_root_hash)
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    #[test]
    fn should_find_lower_bound() {
        // const LARGEST_ERA_ID: usize = 6059;

        const LOWEST_ERA_ID: usize = 0;
        const HIGHEST_ERA_ID: usize = u64::MAX as usize;

        let mut eras = Vec::new();
        eras.push(Some(0)); // 0
        eras.push(Some(1)); // 1
        eras.push(Some(2)); // 2
                            // 3

        let result = super::bisect(0, eras.len() - 1, |idx| find_lower_bound(&eras, idx));
        dbg!(&result);
        assert_eq!(result.as_ref().unwrap().low, 0);

        eras[0] = None;

        let result = super::bisect(0, eras.len() - 1, |idx| find_lower_bound(&eras, idx));
        dbg!(&result);
        assert_eq!(result.as_ref().unwrap().low, 1);

        eras[1] = None;

        let result = super::bisect(0, eras.len() - 1, |idx| find_lower_bound(&eras, idx));
        dbg!(&result);
        assert_eq!(result.as_ref().unwrap().low, 2);
    }

    fn find_lower_bound(eras: &[Option<i32>], idx: usize) -> Result<Ordering, ()> {
        match eras.get(idx).unwrap() {
            Some(val) => {
                if *val == 0 {
                    Ok(Ordering::Equal)
                } else {
                    // Found something, means that idx is too low.
                    Ok(Ordering::Greater)
                }
            }
            None => Ok(Ordering::Less),
        }
    }

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
    }
}
