//! Module containing the purge-era-info migration.

use std::{cell::RefCell, cmp::Ordering, fmt, rc::Rc, time::Instant};

use casper_hashing::Digest;
use casper_types::{EraId, Key};
use num::Num;
use num_traits::NumOps;
use thiserror::Error;

use crate::{
    core::{execution, tracking_copy::TrackingCopy},
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie_store::operations::DeleteResult,
    },
};

/// Binary search an element in a range with added support of a predicate returning a [`Result`].
///
/// Binary searches a range `lower_bound..upper_bound` for a given element.
/// This behaves similarly to [`contains`] on a specified range. If the value is found then
/// `Ok(Result::Left)` is returned, containing the index of the matching element. If there are
/// multiple matches, then any one of the matches could be returned. If the value is not
/// found then `Ok(Result::Right)` is returned, containing the index where a matching element could
/// be inserted while maintaining sorted order.
fn try_binary_search<T, F, E>(lower_bound: T, upper_bound: T, mut f: F) -> Result<Result<T, T>, E>
where
    T: Copy + PartialOrd + Num + NumOps + fmt::Debug,
    F: FnMut(T) -> Result<Ordering, E>,
{
    // INVARIANTS:
    // - 0 <= left <= left + size = right <= self.len()
    // - f returns Less for everything in self[..left]
    // - f returns Greater for everything in self[right..]
    let mut size = upper_bound;
    let mut left = lower_bound;
    let two = T::one() + T::one();
    let mut right = upper_bound;
    while left < right {
        let mid = left + size / two;

        let cmp = f(mid)?;

        match cmp {
            Ordering::Less => {
                left = mid + T::one();
            }
            Ordering::Equal => return Ok(Ok(mid)),
            Ordering::Greater => {
                right = mid;
            }
        }

        size = right - left;
    }

    Ok(Err(left))
}

/// Generic version of `partition_point` that also supports the predicate to return [`Result::Err`].
///
/// Returns the index of the partition point according to the given predicate (the index of the
/// first element of the second partition).
///
/// The slice is assumed to be partitioned according to the given predicate. This means that all
/// elements for which the predicate returns true are at the start of the slice and all elements for
/// which the predicate returns false are at the end. For example, [7, 15, 3, 5, 4, 12, 6] is a
/// partitioned under the predicate x % 2 != 0 (all odd numbers are at the start, all even at the
/// end).
fn try_partition_point<T, F, E>(lower_bound: T, upper_bound: T, mut pred: F) -> Result<T, E>
where
    T: Copy + PartialOrd + Num + NumOps + fmt::Debug,
    F: FnMut(T) -> Result<bool, E>,
{
    let index = try_binary_search(lower_bound, upper_bound, |x| {
        if pred(x)? {
            Ok(Ordering::Less)
        } else {
            Ok(Ordering::Greater)
        }
    })?;

    Ok(index.unwrap_or_else(|i| i))
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
    /// Resulting state root hash after completed migration.
    pub post_state_hash: Digest,
    /// Keys that were deleted.
    pub keys_deleted: Vec<Key>,
}

const LOWER_BOUND_ERA: u64 = 0;

/// Purges exactly `batch_size` of [`Key::EraInfo`] keys from the tip of the store.
pub fn purge_era_info<S>(
    state: &S,
    mut state_root_hash: Digest,
    upper_bound_era_id: u64,
    batch_size: usize,
) -> Result<PurgedEraInfo, Error>
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

    let tc1 = Rc::clone(&tracking_copy);

    let mut lower_bound_era_id_steps = 0usize;

    // Add 1 past upper bound, as the binary search underneath is iterating through exclusive range
    // of [a..b], this addition makes it search through [a..=b].
    let lower_bound_era_id = try_partition_point(LOWER_BOUND_ERA, upper_bound_era_id + 1, |idx| {
        lower_bound_era_id_steps += 1;

        match tc1
            .borrow_mut()
            .get(correlation_id, &Key::EraInfo(EraId::new(idx)))
        {
            Ok(None) => Ok(true),
            Ok(Some(_)) => Ok(false),
            Err(error) => Err(error),
        }
    })
    .map_err(|error| Error::Exec(error.into()))?;

    let max_bound = (lower_bound_era_id + batch_size as u64).min(upper_bound_era_id + 1);

    let keys_to_delete: Vec<Key> = (lower_bound_era_id..max_bound)
        .map(|era_id| Key::EraInfo(EraId::new(era_id)))
        .collect();

    if keys_to_delete.is_empty() {
        // Don't do any work if the range of eras is empty.
        return Ok(PurgedEraInfo {
            post_state_hash: state_root_hash,
            keys_deleted: vec![],
        });
    }

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

    Ok(PurgedEraInfo {
        post_state_hash: state_root_hash,
        keys_deleted: keys_to_delete,
    })
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    #[test]
    fn should_not_panic_binary_search_on_empty_array() {
        let eras: Vec<Option<i32>> = Vec::new();
        let result = try_binary_search(0, 0, |x| -> Result<Ordering, ()> {
            if eras.get(x).unwrap().is_none() {
                Ok(Ordering::Less)
            } else {
                Ok(Ordering::Greater)
            }
        });
        assert_eq!(result, Ok(Err(0)));
    }

    #[test]
    fn should_not_panic_partition_on_empty_array() {
        let eras: Vec<Option<i32>> = Vec::new();
        let result = try_partition_point(0, 0, |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_none())
        });
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn should_not_panic_partition_on_large_search_space() {
        let eras: Vec<Option<i32>> = vec![Some(0), Some(1), None, None, None];

        let mut steps = 0usize;
        let result = try_partition_point(0, usize::MAX, |x| -> Result<bool, ()> {
            steps += 1;
            match eras.get(x) {
                Some(Some(_)) => Ok(true),
                Some(None) | None => Ok(false),
            }
        })
        .unwrap();
        assert_eq!(&eras[..result], vec![Some(0), Some(1)]);
        assert_eq!(steps, 64);
    }

    #[test]
    fn should_find_lower_bound() {
        let mut eras = vec![Some(0), Some(1), Some(2), Some(3)];

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_none())
        });
        assert_eq!(result, Ok(0));
        eras[0] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_none())
        });
        assert_eq!(result, Ok(1));
        eras[1] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_none())
        });
        assert_eq!(result, Ok(2));
        eras[2] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_none())
        });
        assert_eq!(result, Ok(3));
        eras[3] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_none())
        });
        assert_eq!(result, Ok(4));
    }

    #[test]
    fn should_find_upper_bound() {
        let mut eras = vec![Some(0), Some(1), Some(2), Some(3)];

        // let result = super::binary_search(0, eras.len() - 1, |idx| partition_point(&eras, idx));
        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_some())
        });
        assert_eq!(result, Ok(4));
        eras[3] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_some())
        });
        assert_eq!(result, Ok(3));
        eras[2] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_some())
        });
        assert_eq!(result, Ok(2));
        eras[1] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_some())
        });
        assert_eq!(result, Ok(1));
        eras[0] = None;

        let result = try_partition_point(0, eras.len(), |x| -> Result<bool, ()> {
            Ok(eras.get(x).unwrap().is_some())
        });
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn should_run_bisect_on_partially_filled_map() {
        const LARGEST_ERA_ID: usize = 6059;
        const LOWEST_ERA_ID: usize = 0;
        const HIGHEST_ERA_ID: usize = 10000;

        let mut eras = Vec::new();

        for a in 0..LARGEST_ERA_ID {
            eras.push(Some(a));
        }

        let expected_era_id = eras.len() - 1;

        assert!(eras.get(expected_era_id).is_some());
        assert!(eras.get(expected_era_id + 1).is_none());

        for _ in LARGEST_ERA_ID..HIGHEST_ERA_ID {
            eras.push(None);
        }

        let upper_idx =
            try_partition_point(LOWEST_ERA_ID, HIGHEST_ERA_ID, |idx| match eras.get(idx) {
                Some(Some(_)) => Ok(true),
                Some(None) => Ok(false),
                None => Err(()),
            })
            .unwrap();

        assert!(eras.get(upper_idx - 1).unwrap().is_some());
        assert!(eras.get(upper_idx).unwrap().is_none());
    }
}
