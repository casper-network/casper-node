use std::{collections::BTreeMap, iter};

use itertools::Itertools;

use super::{
    highway::Highway,
    state::{self, State, Weight},
    validators::ValidatorIndex,
    vote::{Observation, Panorama, Vote},
};
use crate::{components::consensus::traits::Context, types::Timestamp};

/// A list containing the earliest level-n messages of each member of some committee, for some n.
#[derive(Debug)]
struct Section<'a, C: Context> {
    /// Assigns to each member of a committee the sequence number of the earliest message that
    /// qualifies them for that committee.
    sequence_numbers: BTreeMap<ValidatorIndex, u64>,
    /// A reference to the protocol state this section belongs to.
    state: &'a State<C>,
    // The latest votes that are eligible for the summit.
    latest: &'a BTreeMap<ValidatorIndex, &'a C::Hash>,
}

impl<'a, C: Context> Section<'a, C> {
    /// Creates a section assigning to each validator their level-0 vote, i.e. the oldest vote in
    /// their current streak of votes for `candidate` (and descendants), or `None` if their latest
    /// vote is not for `candidate`.
    fn level0(
        candidate: &'a C::Hash,
        state: &'a State<C>,
        latest: &'a BTreeMap<ValidatorIndex, &'a C::Hash>,
    ) -> Self {
        let height = state.block(candidate).height;
        let to_lvl0vote = |(&idx, &vhash): (&ValidatorIndex, &&'a C::Hash)| {
            state
                .swimlane(vhash)
                .take_while(|(_, vote)| state.find_ancestor(&vote.block, height) == Some(candidate))
                .last()
                .map(|(_, vote)| (idx, vote.seq_number))
        };
        Section {
            sequence_numbers: latest.iter().filter_map(to_lvl0vote).collect(),
            state,
            latest,
        }
    }

    /// Returns a section `s` of votes each of which can see a quorum of votes in `self` by
    /// validators that are part of `s`.
    fn next(&self, quorum: Weight) -> Option<Self> {
        let (committee, _pruned) =
            self.prune_committee(quorum, self.sequence_numbers.keys().cloned().collect());
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
    fn prune_committee(
        &self,
        quorum: Weight,
        mut committee: Vec<ValidatorIndex>,
    ) -> (Vec<ValidatorIndex>, Vec<ValidatorIndex>) {
        let mut pruned = Vec::new();
        loop {
            let sees_quorum = |idx: &ValidatorIndex| {
                self.seen_weight(self.state.vote(self.latest[idx]), &committee) >= quorum
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
    fn committee_quorum(&self, committee: &[ValidatorIndex]) -> Option<Weight> {
        let seen_weight =
            |idx: &ValidatorIndex| self.seen_weight(self.state.vote(self.latest[idx]), committee);
        committee.iter().map(seen_weight).min()
    }

    /// Returns the section containing the earliest vote of each of the `committee` members that
    /// can see a quorum of votes by `committee` members in `self`.
    fn next_from_committee(&self, quorum: Weight, committee: &[ValidatorIndex]) -> Self {
        let find_first_lvl_n = |idx: &ValidatorIndex| {
            let (_, vote) = self
                .state
                .swimlane(self.latest[idx])
                .take_while(|(_, vote)| self.seen_weight(vote, &committee) >= quorum)
                .last()
                .unwrap();
            (*idx, vote.seq_number)
        };
        Section {
            sequence_numbers: committee.iter().map(find_first_lvl_n).collect(),
            state: self.state,
            latest: self.latest,
        }
    }

    /// Returns the total weight of the `committee`'s members whose message in this section is seen
    /// by `vote`.
    fn seen_weight(&self, vote: &Vote<C>, committee: &[ValidatorIndex]) -> Weight {
        let to_weight = |&idx: &ValidatorIndex| self.state.weight(idx);
        let is_seen = |&&idx: &&ValidatorIndex| self.can_see(vote, idx);
        committee.iter().filter(is_seen).map(to_weight).sum()
    }

    /// Returns whether `vote` can see `idx`'s vote in `self`, where `vote` is considered to see
    /// itself.
    fn can_see(&self, vote: &Vote<C>, idx: ValidatorIndex) -> bool {
        self.sequence_numbers.get(&idx).map_or(false, |self_sn| {
            if vote.creator == idx {
                vote.seq_number >= *self_sn
            } else {
                let sees_self_sn = |vhash| self.state.vote(vhash).seq_number >= *self_sn;
                vote.panorama.get(idx).correct().map_or(false, sees_self_sn)
            }
        })
    }
}

/// The result of running the finality detector on a protocol state.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum FinalityOutcome<C: Context> {
    /// No new block has been finalized yet.
    None,
    /// A new block with this consensus value has been finalized.
    Finalized {
        /// The finalized value.
        value: C::ConsensusValue,
        /// The set of newly detected equivocators.
        new_equivocators: Vec<C::ValidatorId>,
        /// Rewards, in trillionths (10^-12) of the total reward for one block.
        /// This is a measure of the value of each validator's contribution to consensus.
        rewards: BTreeMap<C::ValidatorId, u64>,
        /// The timestamp at which this value was proposed.
        timestamp: Timestamp,
    },
    /// The fault tolerance threshold has been exceeded: The number of observed equivocations
    /// invalidates this finality detector's results.
    FttExceeded,
}

/// An incremental finality detector.
///
/// It reuses information between subsequent calls, so it must always be applied to the same
/// `State` instance.
#[derive(Debug)]
pub(crate) struct FinalityDetector<C: Context> {
    /// The most recent known finalized block.
    last_finalized: Option<C::Hash>,
    /// The fault tolerance threshold.
    ftt: Weight,
}

impl<C: Context> FinalityDetector<C> {
    pub(crate) fn new(ftt: Weight) -> Self {
        FinalityDetector {
            last_finalized: None,
            ftt,
        }
    }

    /// Returns the next value, if any has been finalized since the last call.
    // TODO: Iterate this and return multiple finalized blocks.
    // TODO: Verify the consensus instance ID?
    pub(crate) fn run(&mut self, highway: &Highway<C>) -> FinalityOutcome<C> {
        let state = highway.state();
        let fault_w = state.faulty_weight();
        if fault_w >= self.ftt {
            return FinalityOutcome::FttExceeded;
        }
        let bhash = if let Some(bhash) = self.next_finalized(state, fault_w) {
            bhash
        } else {
            return FinalityOutcome::None;
        };
        let to_id =
            |vidx: ValidatorIndex| highway.params().validators.get_by_index(vidx).id().clone();
        let new_equivocators_iter = state.get_new_equivocators(bhash).into_iter();
        let rewards_iter = compute_rewards(state, bhash).into_iter();
        FinalityOutcome::Finalized {
            value: state.block(bhash).value.clone(),
            new_equivocators: new_equivocators_iter.map(to_id).collect(),
            rewards: rewards_iter.map(|(vidx, r)| (to_id(vidx), r)).collect(),
            timestamp: state.vote(bhash).timestamp,
        }
    }

    /// Returns the next block, if any has been finalized since the last call.
    pub(super) fn next_finalized<'a>(
        &mut self,
        state: &'a State<C>,
        fault_w: Weight,
    ) -> Option<&'a C::Hash> {
        let candidate = self.next_candidate(state)?;
        // For `lvl` → ∞, the quorum converges to a fixed value. After level 64, it is closer
        // to that limit than 1/2^-64. This won't make a difference in practice, so there is no
        // point looking for higher summits. It is also too small to be represented in our
        // 64-bit weight type.
        let mut target_lvl = 64;
        while target_lvl > 0 {
            let lvl = self.find_summit(target_lvl, fault_w, candidate, state);
            if lvl == target_lvl {
                self.last_finalized = Some(candidate.clone());
                return Some(candidate);
            }
            // The required quorum increases with decreasing level, so choosing `target_lvl`
            // greater than `lvl` would always yield a summit of level `lvl` or lower.
            target_lvl = lvl;
        }
        None
    }

    /// Returns the number of levels of the highest summit with a quorum that a `target_lvl` summit
    /// would need for the desired FTT. If the returned number is `target_lvl` that means the
    /// `candidate` is finalized. If not, we need to retry with a lower `target_lvl`.
    ///
    /// The faulty validators are considered to be part of any summit, for consistency: That way,
    /// running the finality detector with the same FTT on a later state always returns at least as
    /// many values as on the earlier state, as long as the FTT has not been exceeded.
    fn find_summit(
        &self,
        target_lvl: usize,
        fault_w: Weight,
        candidate: &C::Hash,
        state: &State<C>,
    ) -> usize {
        let total_w = state.total_weight();
        let quorum = self.quorum_for_lvl(target_lvl, total_w) - fault_w;
        let latest = state.panorama().enumerate_correct().collect();
        let sec0 = Section::level0(candidate, &state, &latest);
        let sections_iter = iter::successors(Some(sec0), |sec| sec.next(quorum));
        sections_iter.skip(1).take(target_lvl).count()
    }

    /// Returns the quorum required by a summit with the specified level and the required FTT.
    fn quorum_for_lvl(&self, lvl: usize, total_w: Weight) -> Weight {
        // A level-lvl summit with quorum  total_w/2 + t  has relative FTT  2t(1 − 1/2^lvl). So:
        // quorum = total_w / 2 + ftt / 2 / (1 - 1/2^lvl)
        //        = total_w / 2 + 2^lvl * ftt / 2 / (2^lvl - 1)
        //        = ((2^lvl - 1) total_w + 2^lvl ftt) / (2 * 2^lvl - 2))
        let pow_lvl = 1u128 << lvl;
        let numerator = (pow_lvl - 1) * u128::from(total_w) + pow_lvl * u128::from(self.ftt);
        let denominator = 2 * pow_lvl - 2;
        // Since this is a lower bound for the quorum, we round up when dividing.
        Weight(((numerator + denominator - 1) / denominator) as u64)
    }

    /// Returns the next candidate for finalization, i.e. the lowest block in the fork choice that
    /// has not been finalized yet.
    fn next_candidate<'a>(&self, state: &'a State<C>) -> Option<&'a C::Hash> {
        let fork_choice = state.fork_choice(state.panorama())?;
        state.find_ancestor(fork_choice, self.next_height(state))
    }

    /// Returns the height of the next block that will be finalized.
    fn next_height(&self, state: &State<C>) -> u64 {
        let height_plus_1 = |bhash| state.block(bhash).height + 1;
        self.last_finalized.as_ref().map_or(0, height_plus_1)
    }
}

/// Returns the map of rewards to be paid out when the block `bhash` gets finalized.
fn compute_rewards<C: Context>(state: &State<C>, bhash: &C::Hash) -> BTreeMap<ValidatorIndex, u64> {
    let payout_block = state.block(bhash);
    let payout_vote = state.vote(bhash);
    let panorama = &payout_vote.panorama;
    let opt_parent_vote = payout_block.parent().map(|h| state.vote(h));

    // Returns `true` if the vote carried a block and enough time has passed to pay out rewards.
    let is_rewardable_block = |vh: &&C::Hash| {
        state.opt_block(vh).is_some() && state::reward_time(state.vote(vh)) < payout_vote.timestamp
    };
    // Returns `true` if the vote was already seen by the previous block.
    let is_seen_by_parent = |vh: &C::Hash| {
        // TODO: Not just "correct".
        opt_parent_vote.map_or(false, |vote| state.sees_correct(&vote.panorama, vh))
    };
    // All rewardable blocks not seen by the parent: Rewards for these can be paid now.
    let newly_seen_iter = state
        .justifications_without(panorama, is_seen_by_parent)
        .filter(is_rewardable_block);

    let parent_time = opt_parent_vote.map_or_else(Timestamp::zero, |vote| vote.timestamp);
    // The blocks that weren't rewardable at the parent's timestamp yet, but are now, and are seen.
    let rewards_range_iter = state
        .rewards_range(parent_time..payout_vote.timestamp)
        .filter(|bh| state.sees_correct(panorama, bh)); // TODO: Not just "correct"!

    // All blocks we should pay rewards for, i.e. that are old enough, and that were either not
    // seen by the parent, or not old enough at the parent's timestamp yet.
    let blocks = newly_seen_iter.chain(rewards_range_iter).unique();
    let mut rewards = BTreeMap::new();
    for bh in blocks {
        add_rewards_for(&mut rewards, state, panorama, bh);
    }
    rewards
}

/// Adds the rewards for finalizing the block with hash `proposal_h`.
// TODO: Add a minimum round exponent, to enforce an upper limit for total rewards.
fn add_rewards_for<C: Context>(
    rewards: &mut BTreeMap<ValidatorIndex, u64>,
    state: &State<C>,
    panorama: &Panorama<C>,
    proposal_h: &C::Hash,
) {
    let fault_w = state.faulty_weight_in(panorama);
    let proposal_vote = state.vote(proposal_h);
    let r_id = proposal_vote.round_id();

    // Only consider messages in round `r_id` for the summit. To compute the assigned weight, we
    // also include validators who didn't send a message in that round, but were supposed to.
    let mut assigned_weight = Weight(0);
    let mut latest = BTreeMap::new();
    for (idx, obs) in panorama.enumerate() {
        // Find the validator's latest vote in or before round `r_id`.
        let opt_vote = match obs {
            Observation::Faulty => continue, // Faulty validator aren't assigned to any round.
            Observation::None => None,
            Observation::Correct(latest_vh) => state
                .swimlane(latest_vh)
                .find(|&(_, vote)| vote.round_id() <= r_id),
        };
        match opt_vote {
            None => (), // No vote in or before round `r_id`. This is considered assigned.
            Some((vh, vote)) if vote.round_exp <= r_id.trailing_zeros() => {
                if vote.timestamp >= r_id {
                    latest.insert(idx, vh); // This vote is in round `r_id`. Validator took part.
                }
            }
            Some((_, _)) => continue, // Validator is not assigned to this round.
        }
        assigned_weight += state.weight(idx);
    }

    // Find all level-1 summits. For each validator, store the highest quorum it is a part of.
    let section = Section::level0(proposal_h, state, &latest);
    let (mut committee, _) = section.prune_committee(Weight(1), latest.keys().cloned().collect());
    let mut max_quorum = BTreeMap::new();
    while let Some(quorum) = section.committee_quorum(&committee) {
        // The current committee is a level-1 summit with `quorum`. Try to go higher:
        let (new_committee, pruned) = section.prune_committee(quorum + Weight(1), committee);
        committee = new_committee;
        // Pruned validators are not part of any summit with a higher quorum than this.
        for vidx in &pruned {
            max_quorum.insert(*vidx, quorum);
        }
    }

    // Add the block rewards for each validator who is a member of at least one summit.
    for (vidx, quorum) in max_quorum {
        // If the summit's quorum was not enough to finalize the block, rewards are reduced.
        let finality_factor = if (quorum - fault_w) * 2 > state.total_weight() {
            1_000_000_000_000
        } else {
            u128::from(state.forgiveness_factor())
        };
        // Rewards are proportional to the quorum and to the validator's weight.
        let num = finality_factor * u128::from(quorum) * u128::from(state.weight(vidx));
        let denom = u128::from(assigned_weight) * u128::from(state.total_weight());
        *rewards.entry(vidx).or_insert(0) += (num / denom) as u64;
    }
}

#[allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.
#[cfg(test)]
mod tests {
    use super::{
        super::state::{tests::*, State},
        *,
    };

    #[test]
    fn finality_detector() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new(&[Weight(5), Weight(4), Weight(1)], 0);

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 5
        //        /       \
        // b0: 10           b1: 4
        //        \
        //          c0: 1 — c1: 1
        add_vote!(state, b0, BOB, BOB_SEC, 0; N, N, N; 0xB0);
        add_vote!(state, c0, CAROL, CAROL_SEC, 0; N, b0, N; 0xC0);
        add_vote!(state, c1, CAROL, CAROL_SEC, 1; N, b0, c0; 0xC1);
        add_vote!(state, a0, ALICE, ALICE_SEC, 0; N, b0, N; 0xA0);
        add_vote!(state, a1, ALICE, ALICE_SEC, 1; a0, b0, c1; 0xA1);
        add_vote!(state, b1, BOB, BOB_SEC, 1; a0, b0, N; 0xB1);

        let mut fd4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        let mut fd6 = FinalityDetector::new(Weight(6)); // Fault tolerance 6.

        // The total weight of our validators is 10.
        // A level-k summit with quorum q has relative FTT  2 (q - 10/2) (1 − 1/2^k).
        //
        // `b0`, `a0` are level 0 for `B0`. `a0`, `b1` are level 1.
        // So the fault tolerance of `B0` is 2 * (9 - 10/2) * (1 - 1/2) = 4.
        assert_eq!(None, fd6.next_finalized(&state, 0.into()));
        assert_eq!(Some(&b0), fd4.next_finalized(&state, 0.into()));
        assert_eq!(None, fd4.next_finalized(&state, 0.into()));

        // Adding another level to the summit increases `B0`'s fault tolerance to 6.
        add_vote!(state, _a2, ALICE, ALICE_SEC, 2; a1, b1, c1);
        add_vote!(state, _b2, BOB, BOB_SEC, 2; a1, b1, c1);
        assert_eq!(Some(&b0), fd6.next_finalized(&state, 0.into()));
        assert_eq!(None, fd6.next_finalized(&state, 0.into()));

        // If Bob equivocates, the FTT 4 is exceeded, but she counts as being part of any summit,
        // so `A0` and `A1` get FTT 6. (Bob voted for `A1` and against `B1` in `b2`.)
        assert_eq!(Weight(0), state.faulty_weight());
        add_vote!(state, _e2, BOB, BOB_SEC, 2; a1, b1, c1);
        assert_eq!(Weight(4), state.faulty_weight());
        assert_eq!(Some(&a0), fd6.next_finalized(&state, 4.into()));
        assert_eq!(Some(&a1), fd6.next_finalized(&state, 4.into()));
        assert_eq!(None, fd6.next_finalized(&state, 4.into()));
        Ok(())
    }

    #[test]
    fn equivocators() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fd4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 9 - a2: 5 - a3: 5
        //        /       \      \       \
        // b0: 10           b1: 4  b2: 4  b3: 4
        //        \
        //          c0: 1 — c1: 1
        //               \ c1': 1
        add_vote!(state, b0, BOB, BOB_SEC, 0; N, N, N; 0xB0);
        add_vote!(state, a0, ALICE, ALICE_SEC, 0; N, b0, N; 0xA0);
        add_vote!(state, c0, CAROL, CAROL_SEC, 0; N, b0, N; 0xC0);
        add_vote!(state, _c1, CAROL, CAROL_SEC, 1; N, b0, c0; 0xC1);
        assert_eq!(Weight(0), state.faulty_weight());
        add_vote!(state, _c1_prime, CAROL, CAROL_SEC, 1; N, b0, c0);
        assert_eq!(Weight(1), state.faulty_weight());
        add_vote!(state, b1, BOB, BOB_SEC, 1; a0, b0, N; 0xB1);
        assert_eq!(Some(&b0), fd4.next_finalized(&state, 1.into()));
        add_vote!(state, a1, ALICE, ALICE_SEC, 1; a0, b0, F; 0xA1);
        add_vote!(state, b2, BOB, BOB_SEC, 2; a1, b1, F);
        add_vote!(state, a2, ALICE, ALICE_SEC, 2; a1, b2, F; 0xA2);
        assert_eq!(Some(&a0), fd4.next_finalized(&state, 1.into()));
        // A1 is the first block that sees CAROL equivocating.
        assert_eq!(vec![CAROL], state.get_new_equivocators(&a1));
        assert_eq!(Some(&a1), fd4.next_finalized(&state, 1.into()));
        // Finalize A2. It should not report CAROL as equivocator anymore.
        add_vote!(state, b3, BOB, BOB_SEC, 3; a2, b2, F);
        add_vote!(state, _a3, ALICE, ALICE_SEC, 3; a2, b3, F);
        assert!(state.get_new_equivocators(&a2).is_empty());
        assert_eq!(Some(&a2), fd4.next_finalized(&state, 1.into()));

        // Test that an initial block reports equivocators as well.
        let mut bstate: State<TestContext> = State::new(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fde4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        add_vote!(bstate, _c0, CAROL, CAROL_SEC, 0; N, N, N; 0xB0);
        add_vote!(bstate, _c0_prime, CAROL, CAROL_SEC, 0; N, N, N; 0xB0);
        add_vote!(bstate, a0, ALICE, ALICE_SEC, 0; N, N, F; 0xA0);
        add_vote!(bstate, b0, BOB, BOB_SEC, 0; a0, N, F);
        add_vote!(bstate, _a1, ALICE, ALICE_SEC, 1; a0, b0, F);
        assert_eq!(vec![CAROL], bstate.get_new_equivocators(&a0));
        assert_eq!(Some(&a0), fde4.next_finalized(&bstate, 1.into()));
        Ok(())
    }
}
