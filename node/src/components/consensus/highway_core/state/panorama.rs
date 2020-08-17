use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use crate::components::consensus::{
    highway_core::{
        state::State,
        validators::{ValidatorIndex, ValidatorMap},
    },
    traits::Context,
};

/// The observed behavior of a validator at some point in time.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Observation<C: Context> {
    /// No vote by that validator was observed yet.
    None,
    /// The validator's latest vote.
    Correct(C::Hash),
    /// The validator has been seen
    Faulty,
}

impl<C: Context> Debug for Observation<C>
where
    C::Hash: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Observation::None => write!(f, "N"),
            Observation::Faulty => write!(f, "F"),
            Observation::Correct(hash) => write!(f, "{:?}", hash),
        }
    }
}

impl<C: Context> Observation<C> {
    /// Returns the vote hash, if this is a correct observation.
    pub(crate) fn correct(&self) -> Option<&C::Hash> {
        match self {
            Self::None | Self::Faulty => None,
            Self::Correct(hash) => Some(hash),
        }
    }

    fn is_correct(&self) -> bool {
        match self {
            Self::None | Self::Faulty => false,
            Self::Correct(_) => true,
        }
    }

    pub(crate) fn is_faulty(&self) -> bool {
        match self {
            Self::Faulty => true,
            Self::None | Self::Correct(_) => false,
        }
    }

    /// Returns whether `self` can come later in time than `other`.
    fn geq(&self, state: &State<C>, other: &Observation<C>) -> bool {
        match (self, other) {
            (Observation::Faulty, _) | (_, Observation::None) => true,
            (Observation::Correct(hash0), Observation::Correct(hash1)) => {
                hash0 == hash1 || state.sees_correct(&state.vote(hash0).panorama, hash1)
            }
            (_, _) => false,
        }
    }
}

/// The observed behavior of all validators at some point in time.
pub(crate) type Panorama<C> = ValidatorMap<Observation<C>>;

impl<C: Context> Panorama<C> {
    /// Creates a new, empty panorama.
    pub(crate) fn new(num_validators: usize) -> Panorama<C> {
        Panorama::from(vec![Observation::None; num_validators])
    }

    /// Returns `true` if there is at least one correct observation.
    pub(crate) fn has_correct(&self) -> bool {
        self.iter().any(Observation::is_correct)
    }

    /// Returns an iterator over all hashes of the honest validators' latest messages.
    pub(crate) fn iter_correct(&self) -> impl Iterator<Item = &C::Hash> {
        self.iter().filter_map(Observation::correct)
    }

    /// Returns the correct sequence number for a new vote by `vidx` with this panorama.
    pub(crate) fn next_seq_num(&self, state: &State<C>, vidx: ValidatorIndex) -> u64 {
        let add1 = |vh: &C::Hash| state.vote(vh).seq_number + 1;
        self[vidx].correct().map_or(0, add1)
    }

    /// Returns whether `self` can possibly come later in time than `other`, i.e. it can see
    /// every honest message and every fault seen by `other`.
    pub(super) fn geq(&self, state: &State<C>, other: &Panorama<C>) -> bool {
        let mut pairs_iter = self.iter().zip(other);
        pairs_iter.all(|(obs_self, obs_other)| obs_self.geq(state, obs_other))
    }
}
