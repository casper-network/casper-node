use std::collections::BTreeSet;

use crate::components::consensus::{
    highway_core::{
        state::{State, Unit, Weight},
        validators::{ValidatorIndex, ValidatorMap},
    },
    traits::Context,
};

type Committee = Vec<ValidatorIndex>;

/// A list containing the earliest level-n messages of each member of some committee, for some n.
///
/// A summit is a sequence of committees of validators, where each member of the level-n committee
/// has produced a unit that can see level-(n-1) units by a quorum of the level-n committee.
///
/// The level-n horizon maps each validator of a level-n committee to their earliest level-n unit.
/// From a level-n horizon, the level-(n+1) committee and horizon can be computed.
#[derive(Debug)]
pub(super) struct Horizon<'a, C: Context> {
    /// Assigns to each member of a committee the sequence number of the earliest message that
    /// qualifies them for that committee.
    sequence_numbers: ValidatorMap<Option<u64>>,
    /// A reference to the protocol state this horizon belongs to.
    state: &'a State<C>,
    // The latest units that are eligible for the summit.
    latest: &'a ValidatorMap<Option<&'a C::Hash>>,
}

impl<'a, C: Context> Horizon<'a, C> {
    /// Creates a horizon assigning to each validator their level-0 unit, i.e. the oldest unit in
    /// their current streak of units for `candidate` (and descendants), or `None` if their latest
    /// unit is not for `candidate`.
    pub(super) fn level0(
        candidate: &'a C::Hash,
        state: &'a State<C>,
        latest: &'a ValidatorMap<Option<&'a C::Hash>>,
    ) -> Self {
        let height = state.block(candidate).height;
        let to_lvl0unit = |&maybe_vhash: &Option<&'a C::Hash>| {
            state
                .swimlane(maybe_vhash?)
                .take_while(|(_, unit)| state.find_ancestor(&unit.block, height) == Some(candidate))
                .last()
                .map(|(_, unit)| unit.seq_number)
        };
        Horizon {
            sequence_numbers: latest.iter().map(to_lvl0unit).collect(),
            state,
            latest,
        }
    }

    /// Returns a horizon `s` of units each of which can see a quorum of units in `self` by
    /// validators that are part of `s`.
    pub(super) fn next(&self, quorum: Weight) -> Option<Self> {
        let (committee, _pruned) =
            self.prune_committee(quorum, self.sequence_numbers.keys_some().collect());
        if committee.is_empty() {
            None
        } else {
            Some(self.next_from_committee(quorum, &committee))
        }
    }

    /// Returns the greatest subset of the `committee` of validators whose latest units can see a
    /// quorum of units by the subset in `self`.
    ///
    /// The first returned value is the pruned committee, the second one are the validators that
    /// were pruned.
    ///
    /// Panics if a member of the committee is not in `self.latest`. This can never happen if the
    /// committee was computed from a `Horizon` that originated from the same `level0` as this one.
    pub(super) fn prune_committee(
        &self,
        quorum: Weight,
        mut committee: Committee,
    ) -> (Committee, BTreeSet<ValidatorIndex>) {
        let mut pruned = BTreeSet::new();
        loop {
            let sees_quorum = |idx: &ValidatorIndex| {
                self.seen_weight(self.state.unit(self.latest[*idx].unwrap()), &committee) >= quorum
            };
            let (new_committee, new_pruned): (Vec<_>, Vec<_>) =
                committee.iter().cloned().partition(sees_quorum);
            if new_pruned.is_empty() {
                return (new_committee, pruned);
            }
            pruned.extend(new_pruned);
            committee = new_committee;
        }
    }

    /// Returns the horizon containing the earliest unit of each of the `committee` members that
    /// can see a quorum of units by `committee` members in `self`.
    fn next_from_committee(&self, quorum: Weight, committee: &[ValidatorIndex]) -> Self {
        let find_first_lvl_n = |idx: &ValidatorIndex| {
            self.state
                .swimlane(self.latest[*idx]?)
                .take_while(|(_, unit)| self.seen_weight(unit, committee) >= quorum)
                .last()
                .map(|(_, unit)| (*idx, unit.seq_number))
        };
        let mut sequence_numbers = ValidatorMap::from(vec![None; self.latest.len()]);
        for (vidx, sn) in committee.iter().flat_map(find_first_lvl_n) {
            sequence_numbers[vidx] = Some(sn);
        }
        Horizon {
            sequence_numbers,
            state: self.state,
            latest: self.latest,
        }
    }

    /// Returns the total weight of the `committee`'s members whose message in this horizon is seen
    /// by `unit`.
    fn seen_weight(&self, unit: &Unit<C>, committee: &[ValidatorIndex]) -> Weight {
        let to_weight = |&idx: &ValidatorIndex| self.state.weight(idx);
        let is_seen = |&&idx: &&ValidatorIndex| self.can_see(unit, idx);
        committee.iter().filter(is_seen).map(to_weight).sum()
    }

    /// Returns whether `unit` can see `idx`'s unit in `self`, where `unit` is considered to see
    /// itself.
    fn can_see(&self, unit: &Unit<C>, idx: ValidatorIndex) -> bool {
        self.sequence_numbers[idx].map_or(false, |self_sn| {
            if unit.creator == idx {
                unit.seq_number >= self_sn
            } else {
                let sees_self_sn = |vhash| self.state.unit(vhash).seq_number >= self_sn;
                unit.panorama[idx].correct().map_or(false, sees_self_sn)
            }
        })
    }
}
