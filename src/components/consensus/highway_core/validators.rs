use std::{collections::HashMap, hash::Hash, iter::FromIterator};

use serde::{Deserialize, Serialize};

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
    weight: u64,
    id: VID,
}

impl<VID> From<(VID, u64)> for Validator<VID> {
    fn from((id, weight): (VID, u64)) -> Validator<VID> {
        Validator { id, weight }
    }
}

/// The validator IDs and weight map.
#[derive(Debug)]
pub(crate) struct Validators<VID: Eq + Hash> {
    index_by_id: HashMap<VID, ValidatorIndex>,
    validators: Vec<Validator<VID>>,
}

impl<VID: Eq + Hash> Validators<VID> {
    pub(crate) fn contains(&self, idx: ValidatorIndex) -> bool {
        self.validators.len() as u32 > idx.0
    }
}

impl<VID: Ord + Hash + Clone> FromIterator<(VID, u64)> for Validators<VID> {
    fn from_iter<I: IntoIterator<Item = (VID, u64)>>(ii: I) -> Validators<VID> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_iter() {
        let weights = vec![
            ("Bob".to_string(), 5),
            ("Carol".to_string(), 3),
            ("Alice".to_string(), 4),
        ];
        let validators = Validators::from_iter(weights);
        assert_eq!(ValidatorIndex(0), validators.index_by_id["Alice"]);
        assert_eq!(ValidatorIndex(1), validators.index_by_id["Bob"]);
        assert_eq!(ValidatorIndex(2), validators.index_by_id["Carol"]);
    }
}
