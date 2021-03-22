use std::{
    collections::HashMap,
    fmt,
    hash::Hash,
    iter::FromIterator,
    ops::{Add, Index, IndexMut},
    slice, vec,
};

use datasize::DataSize;
use derive_more::{AsRef, From};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use super::Weight;
use crate::utils::ds;

/// The index of a validator, in a list of all validators, ordered by ID.
#[derive(
    Copy, Clone, DataSize, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize,
)]
pub(crate) struct ValidatorIndex(pub(crate) u32);

impl From<u32> for ValidatorIndex {
    fn from(idx: u32) -> Self {
        ValidatorIndex(idx)
    }
}

/// Information about a validator: their ID and weight.
#[derive(Clone, DataSize, Debug, Eq, PartialEq)]
pub(crate) struct Validator<VID> {
    weight: Weight,
    id: VID,
    banned: bool,
}

impl<VID, W: Into<Weight>> From<(VID, W)> for Validator<VID> {
    fn from((id, weight): (VID, W)) -> Validator<VID> {
        Validator {
            id,
            weight: weight.into(),
            banned: false,
        }
    }
}

impl<VID> Validator<VID> {
    pub(crate) fn id(&self) -> &VID {
        &self.id
    }

    pub(crate) fn weight(&self) -> Weight {
        self.weight
    }
}

/// The validator IDs and weight map.
#[derive(Debug, DataSize, Clone)]
pub(crate) struct Validators<VID>
where
    VID: Eq + Hash,
{
    index_by_id: HashMap<VID, ValidatorIndex>,
    validators: Vec<Validator<VID>>,
}

impl<VID: Eq + Hash> Validators<VID> {
    pub(crate) fn total_weight(&self) -> Weight {
        self.validators.iter().fold(Weight(0), |sum, v| {
            sum.checked_add(v.weight())
                .expect("total weight must be < 2^64")
        })
    }

    pub(crate) fn get_index(&self, id: &VID) -> Option<ValidatorIndex> {
        self.index_by_id.get(id).cloned()
    }

    /// Returns validator ID by index, or `None` if it doesn't exist.
    pub(crate) fn id(&self, idx: ValidatorIndex) -> Option<&VID> {
        self.validators.get(idx.0 as usize).map(Validator::id)
    }

    /// Returns an iterator over all validators, sorted by ID.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Validator<VID>> {
        self.validators.iter()
    }

    /// Marks the validator with that ID as banned, if it exists.
    pub(crate) fn ban(&mut self, vid: &VID) {
        if let Some(idx) = self.get_index(vid) {
            self.validators[idx.0 as usize].banned = true;
        }
    }

    /// Returns an iterator of all indices of banned validators.
    pub(crate) fn iter_banned_idx(&self) -> impl Iterator<Item = ValidatorIndex> + '_ {
        self.iter()
            .enumerate()
            .filter(|(_, v)| v.banned)
            .map(|(idx, _)| ValidatorIndex::from(idx as u32))
    }

    pub(crate) fn enumerate_ids<'a>(&'a self) -> impl Iterator<Item = (ValidatorIndex, &'a VID)> {
        let to_idx =
            |(idx, v): (usize, &'a Validator<VID>)| (ValidatorIndex::from(idx as u32), v.id());
        self.iter().enumerate().map(to_idx)
    }

    /// Returns the size of validator list.
    pub(crate) fn len(&self) -> usize {
        self.validators.len()
    }
}

impl<VID: Ord + Hash + Clone, W: Into<Weight>> FromIterator<(VID, W)> for Validators<VID> {
    fn from_iter<I: IntoIterator<Item = (VID, W)>>(ii: I) -> Validators<VID> {
        let mut validators: Vec<_> = ii.into_iter().map(Validator::from).collect();
        validators.sort_by_cached_key(|val| val.id.clone());
        let index_by_id = validators
            .iter()
            .enumerate()
            .map(|(idx, val)| (val.id.clone(), ValidatorIndex(idx as u32)))
            .collect();
        Validators {
            index_by_id,
            validators,
        }
    }
}

impl<VID: Ord + Hash + fmt::Debug> fmt::Display for Validators<VID> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Validators: index, ID, weight")?;
        for (i, val) in self.validators.iter().enumerate() {
            writeln!(f, "{:3}, {:?}, {}", i, val.id(), val.weight().0)?
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, AsRef, From, Hash)]
pub(crate) struct ValidatorMap<T>(Vec<T>);

impl<T> fmt::Display for ValidatorMap<Option<T>>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let view = self
            .0
            .iter()
            .map(|maybe_el| match maybe_el {
                None => "N".to_string(),
                Some(el) => format!("{}", el),
            })
            .join(", ");
        write!(f, "ValidatorMap({})", view)?;
        Ok(())
    }
}

impl<T> DataSize for ValidatorMap<T>
where
    T: DataSize,
{
    const IS_DYNAMIC: bool = Vec::<T>::IS_DYNAMIC;

    const STATIC_HEAP_SIZE: usize = Vec::<T>::STATIC_HEAP_SIZE;

    fn estimate_heap_size(&self) -> usize {
        ds::vec_sample(&self.0)
    }
}

impl<T> ValidatorMap<T> {
    /// Returns the value for the given validator. Panics if the index is out of range.
    pub(crate) fn get(&self, idx: ValidatorIndex) -> &T {
        &self.0[idx.0 as usize]
    }

    /// Returns the number of values. This must equal the number of validators.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns an iterator over all values.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }

    /// Returns an iterator over mutable references to all values.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.0.iter_mut()
    }

    /// Returns an iterator over all values, by validator index.
    pub(crate) fn enumerate(&self) -> impl Iterator<Item = (ValidatorIndex, &T)> {
        self.iter()
            .enumerate()
            .map(|(idx, value)| (ValidatorIndex(idx as u32), value))
    }

    /// Returns an iterator over all validator indices.
    pub(crate) fn keys(&self) -> impl Iterator<Item = ValidatorIndex> {
        (0..self.len()).map(|idx| ValidatorIndex(idx as u32))
    }

    /// Binary searches this sorted `ValidatorMap` for `x`.
    ///
    /// If the value is found, `Ok` is returned, containing the index, otherwise `Err`, with the
    /// first index at which the value is greater than `x`.
    pub fn binary_search(&self, x: &T) -> Result<ValidatorIndex, ValidatorIndex>
    where
        T: Ord,
    {
        self.0
            .binary_search(x)
            .map(|i| ValidatorIndex(i as u32))
            .map_err(|i| ValidatorIndex(i as u32))
    }
}

impl<T> IntoIterator for ValidatorMap<T> {
    type Item = T;
    type IntoIter = vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T> FromIterator<T> for ValidatorMap<T> {
    fn from_iter<I: IntoIterator<Item = T>>(ii: I) -> ValidatorMap<T> {
        ValidatorMap(ii.into_iter().collect())
    }
}

impl<T> Index<ValidatorIndex> for ValidatorMap<T> {
    type Output = T;

    fn index(&self, vidx: ValidatorIndex) -> &T {
        &self.0[vidx.0 as usize]
    }
}

impl<T> IndexMut<ValidatorIndex> for ValidatorMap<T> {
    fn index_mut(&mut self, vidx: ValidatorIndex) -> &mut T {
        &mut self.0[vidx.0 as usize]
    }
}

impl<'a, T> IntoIterator for &'a ValidatorMap<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<Rhs, T: Copy + Add<Rhs, Output = T>> Add<ValidatorMap<Rhs>> for ValidatorMap<T> {
    type Output = ValidatorMap<T>;
    fn add(mut self, rhs: ValidatorMap<Rhs>) -> Self::Output {
        self.0
            .iter_mut()
            .zip(rhs)
            .for_each(|(lhs_val, rhs_val)| *lhs_val = *lhs_val + rhs_val);
        self
    }
}

impl<T> ValidatorMap<Option<T>> {
    /// Returns the keys of all validators whose value is `Some`.
    pub(crate) fn keys_some(&self) -> impl Iterator<Item = ValidatorIndex> + '_ {
        self.iter_some().map(|(vidx, _)| vidx)
    }

    /// Returns an iterator over all values that are present, together with their index.
    pub(crate) fn iter_some(&self) -> impl Iterator<Item = (ValidatorIndex, &T)> + '_ {
        self.enumerate()
            .filter_map(|(vidx, opt)| opt.as_ref().map(|val| (vidx, val)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_iter() {
        let weights = vec![
            ("Bob".to_string(), 5u64),
            ("Carol".to_string(), 3),
            ("Alice".to_string(), 4),
        ];
        let validators = Validators::from_iter(weights);
        assert_eq!(ValidatorIndex(0), validators.index_by_id["Alice"]);
        assert_eq!(ValidatorIndex(1), validators.index_by_id["Bob"]);
        assert_eq!(ValidatorIndex(2), validators.index_by_id["Carol"]);
    }
}
