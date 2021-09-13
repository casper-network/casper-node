use std::fmt::Debug;

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::components::consensus::{
    highway_core::{
        state::{Observation, Panorama, State},
        validators::ValidatorMap,
    },
    traits::Context,
};

pub(crate) type IndexPanorama = ValidatorMap<IndexObservation>;

/// The observed behavior of a validator at some point in time.
#[derive(Clone, DataSize, Eq, PartialEq, Serialize, Deserialize, Hash)]
pub(crate) enum IndexObservation {
    None,
    Faulty,
    Correct(u64),
}

impl Debug for IndexObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexObservation::None => write!(f, "N"),
            IndexObservation::Faulty => write!(f, "F"),
            IndexObservation::Correct(seq_num) => write!(f, "{:?}", seq_num),
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
            ValidatorMap::from(vec![IndexObservation::None; panorama.len()]);
        for (vid, obs) in panorama.enumerate() {
            let index_obs = match obs {
                Observation::None => IndexObservation::None,
                Observation::Correct(hash) => state.maybe_unit(hash).map_or_else(
                    || {
                        error!(?hash, "expected unit to exist in the local protocol state");
                        IndexObservation::None
                    },
                    |unit| IndexObservation::Correct(unit.seq_number),
                ),
                Observation::Faulty => IndexObservation::Faulty,
            };
            validator_map[vid] = index_obs;
        }
        validator_map
    }
}
