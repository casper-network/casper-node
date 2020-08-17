use std::fmt::Debug;

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::{
        highway_core::{
            state::State,
            validators::{ValidatorIndex, ValidatorMap},
        },
        traits::Context,
    },
    types::Timestamp,
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
                hash0 == hash1 || state.vote(hash0).panorama.sees_correct(state, hash1)
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

    /// Returns `true` if `self` sees the creator of `hash` as correct, and sees that vote.
    pub(crate) fn sees_correct(&self, state: &State<C>, hash: &C::Hash) -> bool {
        let vote = state.vote(hash);
        let can_see = |latest_hash: &C::Hash| {
            Some(hash) == state.find_in_swimlane(latest_hash, vote.seq_number)
        };
        self.get(vote.creator).correct().map_or(false, can_see)
    }

    /// Merges two panoramas into a new one.
    pub(crate) fn merge(&self, state: &State<C>, other: &Panorama<C>) -> Panorama<C> {
        let merge_obs = |observations: (&Observation<C>, &Observation<C>)| match observations {
            (Observation::Faulty, _) | (_, Observation::Faulty) => Observation::Faulty,
            (Observation::None, obs) | (obs, Observation::None) => obs.clone(),
            (obs0, Observation::Correct(vh1)) if self.sees_correct(state, vh1) => obs0.clone(),
            (Observation::Correct(vh0), obs1) if other.sees_correct(state, vh0) => obs1.clone(),
            (Observation::Correct(_), Observation::Correct(_)) => Observation::Faulty,
        };
        let observations = self.iter().zip(other).map(merge_obs).collect_vec();
        Panorama::from(observations)
    }

    /// Returns the panorama seeing all votes seen by `self` with a timestamp no later than
    /// `timestamp`. Accusations are preserved regardless of the evidence's timestamp.
    pub(crate) fn cutoff(&self, state: &State<C>, timestamp: Timestamp) -> Panorama<C> {
        let obs_cutoff = |obs: &Observation<C>| match obs {
            Observation::Correct(vhash) => state
                .swimlane(vhash)
                .find(|(_, vote)| vote.timestamp <= timestamp)
                .map(|(vh, _)| vh.clone())
                .map_or(Observation::None, Observation::Correct),
            obs @ Observation::None | obs @ Observation::Faulty => obs.clone(),
        };
        Panorama::from(self.iter().map(obs_cutoff).collect_vec())
    }

    /// Returns whether `self` can possibly come later in time than `other`, i.e. it can see
    /// every honest message and every fault seen by `other`.
    pub(super) fn geq(&self, state: &State<C>, other: &Panorama<C>) -> bool {
        let mut pairs_iter = self.iter().zip(other);
        pairs_iter.all(|(obs_self, obs_other)| obs_self.geq(state, obs_other))
    }

    /// Returns whether `self` is valid, i.e. it contains the latest votes of some substate.
    pub(super) fn is_valid(&self, state: &State<C>) -> bool {
        self.enumerate().all(|(idx, observation)| {
            match observation {
                Observation::None => true,
                Observation::Faulty => state.has_evidence(idx),
                Observation::Correct(hash) => match state.opt_vote(hash) {
                    Some(vote) => vote.creator == idx && self.geq(state, &vote.panorama),
                    None => false, // Unknown vote. Not a substate of `state`.
                },
            }
        })
    }
}
