use std::{
    collections::HashMap,
    hash::Hash,
    iter::FromIterator,
    ops::{Add, Index, IndexMut},
    slice, vec,
};

use derive_more::{AsRef, From};
use serde::{Deserialize, Serialize};

use super::Weight;

/// The index of a validator, in a list of all validators, ordered by ID.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub(crate) struct ValidatorIndex(pub(crate) u32);

impl From<u32> for ValidatorIndex {
    fn from(idx: u32) -> Self {
        ValidatorIndex(idx)
    }
}

/// Information about a validator: their ID and weight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Validator<VID> {
    weight: Weight,
    id: VID,
}

impl<VID, W: Into<Weight>> From<(VID, W)> for Validator<VID> {
    fn from((id, weight): (VID, W)) -> Validator<VID> {
        Validator {
            id,
            weight: weight.into(),
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
#[derive(Debug, Clone)]
pub(crate) struct Validators<VID: Eq + Hash> {
    index_by_id: HashMap<VID, ValidatorIndex>,
    validators: Vec<Validator<VID>>,
}

impl<VID: Eq + Hash> Validators<VID> {
    pub(crate) fn contains(&self, idx: ValidatorIndex) -> bool {
        self.validators.len() as u32 > idx.0
    }

    pub(crate) fn total_weight(&self) -> Weight {
        self.validators.iter().map(|v| v.weight()).sum()
    }

    pub(crate) fn get_index(&self, id: &VID) -> ValidatorIndex {
        *self.index_by_id.get(id).unwrap()
    }

    /// Returns validator at index.
    /// Expects that idx has been validated before calling this function.
    pub(crate) fn get_by_index(&self, idx: ValidatorIndex) -> &Validator<VID> {
        &self.validators.get(idx.0 as usize).unwrap()
    }

    pub(crate) fn enumerate(&self) -> impl Iterator<Item = (ValidatorIndex, &Validator<VID>)> {
        self.validators
            .iter()
            .enumerate()
            .map(|(idx, v)| (ValidatorIndex(idx as u32), v))
    }
}

impl<VID: Ord + Hash + Clone, W: Into<Weight>> FromIterator<(VID, W)> for Validators<VID> {
    fn from_iter<I: IntoIterator<Item = (VID, W)>>(ii: I) -> Validators<VID> {
        let mut validators: Vec<_> = ii.into_iter().map(Validator::from).collect();
        validators.sort_by(|val0, val1| val0.id.cmp(&val1.id));
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, AsRef, From)]
pub(crate) struct ValidatorMap<T>(Vec<T>);

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

    /// Returns an iterator over all values, by validator index.
    pub(crate) fn enumerate(&self) -> impl Iterator<Item = (ValidatorIndex, &T)> {
        self.iter()
            .enumerate()
            .map(|(idx, value)| (ValidatorIndex(idx as u32), value))
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
    pub(crate) fn keys_some<'a>(&'a self) -> impl Iterator<Item = ValidatorIndex> + 'a {
        self.enumerate()
            .filter(|(_, opt)| opt.is_some())
            .map(|(vidx, _)| vidx)
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
