use std::fmt::Debug;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::components::consensus::{
    highway_core::{
        state::{Observation, Panorama, State},
        validators::ValidatorMap,
    },
    traits::Context,
};

pub(crate) type IndexPanorama = ValidatorMap<IndexObservation>;

/// The observed behavior of a validator at some point in time.
#[derive(Clone, Copy, DataSize, Eq, PartialEq, Serialize, Deserialize, Hash)]
pub(crate) enum IndexObservation {
    /// We have evidence that the validator is faulty.
    Faulty,
    /// We have that number of units from the validator.
    /// That is also the sequence number of the first unit we are still missing.
    Count(u64),
}

impl Debug for IndexObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexObservation::Faulty => write!(f, "F"),
            IndexObservation::Count(count) => write!(f, "{:?}", count),
        }
    }
}

impl IndexPanorama {
    /// Creates an instance of `IndexPanorama` out of a panorama.
    pub(crate) fn from_panorama<'a, C: Context>(
        panorama: &'a Panorama<C>,
        state: &'a State<C>,
    ) -> Self {
        let mut validator_map: ValidatorMap<IndexObservation> =
            ValidatorMap::from(vec![IndexObservation::Count(0); panorama.len()]);
        for (vid, obs) in panorama.enumerate() {
            let index_obs = match obs {
                Observation::None => IndexObservation::Count(0),
                Observation::Correct(hash) => IndexObservation::Count(
                    state
                        .maybe_unit(hash)
                        .map_or(0, |unit| unit.seq_number.saturating_add(1)),
                ),
                Observation::Faulty => IndexObservation::Faulty,
            };
            validator_map[vid] = index_obs;
        }
        validator_map
    }
}
