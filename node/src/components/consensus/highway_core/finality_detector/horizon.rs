use std::collections::BTreeSet;

use crate::components::consensus::{
    highway_core::{
        state::{State, Vote, Weight},
        validators::{ValidatorIndex, ValidatorMap},
    },
    traits::Context,
};

type Committee = Vec<ValidatorIndex>;

/// A list containing the earliest level-n messages of each member of some committee, for some n.
#[derive(Debug)]
pub(super) struct Horizon<'a, C: Context> {
    /// Assigns to each member of a committee the sequence number of the earliest message that
    /// qualifies them for that committee.
    sequence_numbers: ValidatorMap<Option<u64>>,
    /// A reference to the protocol state this horizon belongs to.
    state: &'a State<C>,
    // The latest votes that are eligible for the summit.
    latest: &'a ValidatorMap<Option<&'a C::Hash>>,
}

impl<'a, C: Context> Horizon<'a, C> {
    /// Creates a horizon assigning to each validator their level-0 vote, i.e. the oldest vote in
    /// their current streak of votes for `candidate` (and descendants), or `None` if their latest
    /// vote is not for `candidate`.
    pub(super) fn level0(
        candidate: &'a C::Hash,
        state: &'a State<C>,
        latest: &'a ValidatorMap<Option<&'a C::Hash>>,
    ) -> Self {
        let height = state.block(candidate).height;
        let to_lvl0vote = |&opt_vhash: &Option<&'a C::Hash>| {
            state
                .swimlane(opt_vhash?)
                .take_while(|(_, vote)| state.find_ancestor(&vote.block, height) == Some(candidate))
                .last()
                .map(|(_, vote)| vote.seq_number)
        };
        Horizon {
            sequence_numbers: latest.iter().map(to_lvl0vote).collect(),
            state,
            latest,
        }
    }

    /// Returns a horizon `s` of votes each of which can see a quorum of votes in `self` by
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

    /// Returns the greatest subset of the `committee` of validators whose latest votes can see a
    /// quorum of votes by the subset in `self`.
    ///
    /// The first returned value is the pruned committee, the second one are the validators that
    /// were pruned.
    pub(super) fn prune_committee(
        &self,
        quorum: Weight,
        mut committee: Committee,
    ) -> (Committee, BTreeSet<ValidatorIndex>) {
        let mut pruned = BTreeSet::new();
        loop {
            let sees_quorum = |idx: &ValidatorIndex| {
                self.seen_weight(self.state.vote(self.latest[*idx].unwrap()), &committee) >= quorum
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

    /// The maximal quorum for which this is a committee, i.e. the minimum seen weight of the
    /// members.
    pub(super) fn committee_quorum(&self, committee: &[ValidatorIndex]) -> Option<Weight> {
        let seen_weight = |idx: &ValidatorIndex| {
            self.seen_weight(self.state.vote(self.latest[*idx].unwrap()), committee)
        };
        committee.iter().map(seen_weight).min()
    }

    /// Returns the horizon containing the earliest vote of each of the `committee` members that
    /// can see a quorum of votes by `committee` members in `self`.
    fn next_from_committee(&self, quorum: Weight, committee: &[ValidatorIndex]) -> Self {
        let find_first_lvl_n = |idx: &ValidatorIndex| {
            self.state
                .swimlane(self.latest[*idx]?)
                .take_while(|(_, vote)| self.seen_weight(vote, &committee) >= quorum)
                .last()
                .map(|(_, vote)| (*idx, vote.seq_number))
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
    /// by `vote`.
    fn seen_weight(&self, vote: &Vote<C>, committee: &[ValidatorIndex]) -> Weight {
        let to_weight = |&idx: &ValidatorIndex| self.state.weight(idx);
        let is_seen = |&&idx: &&ValidatorIndex| self.can_see(vote, idx);
        committee.iter().filter(is_seen).map(to_weight).sum()
    }

    /// Returns whether `vote` can see `idx`'s vote in `self`, where `vote` is considered to see
    /// itself.
    fn can_see(&self, vote: &Vote<C>, idx: ValidatorIndex) -> bool {
        self.sequence_numbers[idx].map_or(false, |self_sn| {
            if vote.creator == idx {
                vote.seq_number >= self_sn
            } else {
                let sees_self_sn = |vhash| self.state.vote(vhash).seq_number >= self_sn;
                vote.panorama.get(idx).correct().map_or(false, sees_self_sn)
            }
        })
    }
}
