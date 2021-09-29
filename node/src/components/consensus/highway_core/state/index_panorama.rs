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
    /// The next sequence number we need, i.e. the lowest one that is missing from our protocol
    /// state. This is equal to the total number of units we have from that validator, and one more
    /// than the highest sequence number we have.
    NextSeq(u64),
}

impl Debug for IndexObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexObservation::Faulty => write!(f, "F"),
            IndexObservation::NextSeq(next_seq) => write!(f, "{:?}", next_seq),
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
            ValidatorMap::from(vec![IndexObservation::NextSeq(0); panorama.len()]);
        for (vid, obs) in panorama.enumerate() {
            let index_obs = match obs {
                Observation::None => IndexObservation::NextSeq(0),
                Observation::Correct(hash) => IndexObservation::NextSeq(
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
