//! Module containing the purge-era-info migration.

use std::{cell::RefCell, cmp::Ordering, fmt, ops::Shr, rc::Rc, time::Instant};

use casper_hashing::Digest;
use casper_types::{EraId, Key};
use num::Num;
use thiserror::Error;

use crate::{
    core::{execution, tracking_copy::TrackingCopy},
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie_store::operations::DeleteResult,
    },
};

use super::ActionSuccess;

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
    /// Keys that were deleted.
    pub keys_deleted: Vec<Key>,
}

const LOWER_BOUND_ERA: u64 = 0;

/// Purges [`Key::EraInfo`] keys from the tip of the store and writes only single key with the
/// latest era into a stable key [`Key::EraSummary`].
pub fn purge_era_info<S>(
    state: &S,
    mut state_root_hash: Digest,
    upper_bound_era: u64,
    batch_size: usize,
) -> Result<ActionSuccess, Error>
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
            Ok(Some(_era_id)) => {
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

    // our upper (exclusive) bound should be whichever is lower => low + batch_size or upper_bound_era + 1
    let max_bound = (result.low + batch_size as u64).min(upper_bound_era + 1);
    let keys_to_delete: Vec<Key> = (result.low..max_bound)
        .map(|era_id| Key::EraInfo(EraId::new(era_id)))
        .collect();

    dbg!(&keys_to_delete);

    if keys_to_delete.is_empty() {
        // Don't do any work if not keys are present in the global state.
        return Ok(ActionSuccess::PurgeEraInfo {
            post_state_hash: state_root_hash,
            action_result: PurgedEraInfo {
                keys_deleted: vec![],
            },
        });
    }

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

    Ok(ActionSuccess::PurgeEraInfo {
        post_state_hash: state_root_hash,
        action_result: PurgedEraInfo {
            keys_deleted: keys_to_delete,
        },
    })
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
                Some(Some(_val)) => {
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
