use std::{collections::BTreeMap, iter};

use super::{
    state::{State, Weight},
    validators::ValidatorIndex,
    vote::{Observation, Vote},
};
use crate::components::consensus::traits::{ConsensusValueT, Context};

/// A list containing the earliest level-n messages of each member of some committee, for some n.
#[derive(Debug)]
struct Section<'a, C: Context> {
    /// Assigns to each member of a committee the sequence number of the earliest message that
    /// qualifies them for that committee.
    sequence_numbers: BTreeMap<ValidatorIndex, u64>,
    /// A reference to the protocol state this section belongs to.
    state: &'a State<C>,
}

impl<'a, C: Context> Section<'a, C> {
    /// Creates a section assigning to each validator their level-0 vote, i.e. the oldest vote in
    /// their current streak of votes for `candidate` (and descendants), or `None` if their latest
    /// vote is not for `candidate`.
    fn level0(candidate: &'a C::Hash, state: &'a State<C>) -> Self {
        let height = state.block(candidate).height;
        let to_lvl0vote = |(idx, vhash): (ValidatorIndex, &'a C::Hash)| {
            state
                .swimlane(vhash)
                .take_while(|(_, vote)| state.find_ancestor(&vote.block, height) == Some(candidate))
                .last()
                .map(|(_, vote)| (idx, vote.seq_number))
        };
        let correct_votes = state.panorama().enumerate_correct();
        Section {
            sequence_numbers: correct_votes.filter_map(to_lvl0vote).collect(),
            state,
        }
    }

    /// Returns a section `s` of votes each of which can see a quorum of votes in `self` by
    /// validators that are part of `s`.
    fn next(&self, quorum: Weight) -> Option<Self> {
        let committee = self.pruned_committee(quorum);
        if committee.is_empty() {
            None
        } else {
            Some(self.next_from_committee(quorum, &committee))
        }
    }

    /// Returns the greatest committee of validators whose latest votes can see a quorum of votes
    /// by the committee in `self`.
    fn pruned_committee(&self, quorum: Weight) -> Vec<ValidatorIndex> {
        let mut committee: Vec<ValidatorIndex> = self.sequence_numbers.keys().cloned().collect();
        loop {
            let old_comm = committee;
            let sees_quorum = |&idx: &ValidatorIndex| {
                let vhash = self.state.panorama().get(idx).correct().unwrap();
                self.seen_weight(self.state.vote(vhash), &old_comm) >= quorum
            };
            committee = old_comm.iter().cloned().filter(sees_quorum).collect();
            if committee == old_comm {
                return committee;
            }
        }
    }

    /// Returns the section containing the earliest vote of each of the `committee` members that
    /// can see a quorum of votes by `committee` members in `self`.
    fn next_from_committee(&self, quorum: Weight, committee: &[ValidatorIndex]) -> Self {
        let find_first_lvl_n = |&idx: &ValidatorIndex| {
            let (_, vote) = self
                .state
                .swimlane(self.state.panorama().get(idx).correct().unwrap())
                .take_while(|(_, vote)| self.seen_weight(vote, &committee) >= quorum)
                .last()
                .unwrap();
            (idx, vote.seq_number)
        };
        Section {
            sequence_numbers: committee.iter().map(find_first_lvl_n).collect(),
            state: self.state,
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
            if vote.sender == idx {
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
pub(crate) enum FinalityResult<V: ConsensusValueT, VID> {
    /// No new block has been finalized yet.
    None,
    /// A new block with these consensus values has been finalized.
    /// Second vectors is a list of indexes of validators that equivocated.
    Finalized(Vec<V>, Vec<VID>),
    /// The fault tolerance threshold has been exceeded: The number of observed equivocation
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

    /// Returns the next batch of values, if any has been finalized since the last call.
    // TODO: Iterate this and return multiple finalized blocks.
    // TODO: Verify the consensus instance ID?
    pub(crate) fn run(
        &mut self,
        state: &State<C>,
    ) -> FinalityResult<C::ConsensusValue, ValidatorIndex> {
        let fault_w: Weight = state
            .panorama()
            .iter()
            .zip(state.weights())
            .filter(|(obs, _)| **obs == Observation::Faulty)
            .map(|(_, w)| *w)
            .sum();
        if fault_w >= self.ftt {
            return FinalityResult::FttExceeded;
        }
        if let Some(candidate) = self.next_candidate(state) {
            // For `lvl` → ∞, the quorum converges to a fixed value. After level 64, it is closer
            // to that limit than 1/2^-64. This won't make a difference in practice, so there is no
            // point looking for higher summits. It is also too small to be represented in our
            // 64-bit weight type.
            let mut target_lvl = 64;
            while target_lvl > 0 {
                let lvl = self.find_summit(target_lvl, fault_w, candidate, state);
                if lvl == target_lvl {
                    self.last_finalized = Some(candidate.clone());
                    let new_equivocators = state.get_new_equivocators(&candidate);
                    return FinalityResult::Finalized(
                        state.block(candidate).values.clone(),
                        new_equivocators,
                    );
                }
                // The required quorum increases with decreasing level, so choosing `target_lvl`
                // greater than `lvl` would always yield a summit of level `lvl` or lower.
                target_lvl = lvl;
            }
        }
        FinalityResult::None
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
        let sec0 = Section::level0(candidate, &state);
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
        let numerator = (pow_lvl - 1) * (total_w.0 as u128) + pow_lvl * (self.ftt.0 as u128);
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

#[cfg(test)]
mod tests {
    use super::{
        super::state::{tests::*, AddVoteError, State},
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
        assert_eq!(FinalityResult::None, fd6.run(&state));
        assert_eq!(
            FinalityResult::Finalized(vec![0xB0], vec![]),
            fd4.run(&state)
        );
        assert_eq!(FinalityResult::None, fd4.run(&state));

        // Adding another level to the summit increases `B0`'s fault tolerance to 6.
        add_vote!(state, _a2, ALICE, ALICE_SEC, 2; a1, b1, c1);
        add_vote!(state, _b2, BOB, BOB_SEC, 2; a1, b1, c1);
        assert_eq!(
            FinalityResult::Finalized(vec![0xB0], vec![]),
            fd6.run(&state)
        );
        assert_eq!(FinalityResult::None, fd6.run(&state));

        // If Alice equivocates, the FTT 4 is exceeded, but she counts as being part of any summit,
        // so `A0` and `A1` get FTT 6. (Bob voted for `A1` and against `B1` in `b2`.)
        add_vote!(state, _e2, BOB, BOB_SEC, 2; a1, b1, c1);
        assert_eq!(FinalityResult::FttExceeded, fd4.run(&state));
        assert_eq!(
            FinalityResult::Finalized(vec![0xA0], vec![]),
            fd6.run(&state)
        );
        assert_eq!(
            FinalityResult::Finalized(vec![0xA1], vec![]),
            fd6.run(&state)
        );
        assert_eq!(FinalityResult::None, fd6.run(&state));
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
        add_vote!(state, _c1_prime, CAROL, CAROL_SEC, 1; N, b0, c0);
        add_vote!(state, b1, BOB, BOB_SEC, 1; a0, b0, N; 0xB1);
        assert_eq!(
            FinalityResult::Finalized(vec![0xB0], vec![]),
            fd4.run(&state)
        );
        add_vote!(state, a1, ALICE, ALICE_SEC, 1; a0, b0, F; 0xA1);
        add_vote!(state, b2, BOB, BOB_SEC, 2; a1, b1, F);
        add_vote!(state, a2, ALICE, ALICE_SEC, 2; a1, b2, F; 0xA2);
        assert_eq!(
            FinalityResult::Finalized(vec![0xA0], vec![]),
            fd4.run(&state)
        );
        // A1 is the first block that sees CAROL equivocating.
        assert_eq!(
            FinalityResult::Finalized(vec![0xA1], vec![CAROL]),
            fd4.run(&state)
        );
        // Finalize A2. It should not report CAROL as equivocator anymore.
        add_vote!(state, b3, BOB, BOB_SEC, 3; a2, b2, F);
        add_vote!(state, _a3, ALICE, ALICE_SEC, 3; a2, b3, F);
        assert_eq!(
            FinalityResult::Finalized(vec![0xA2], vec![]),
            fd4.run(&state)
        );

        // Test that an initial block reports equivocators as well.
        let mut bstate: State<TestContext> = State::new(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fde4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        add_vote!(bstate, c0, CAROL, CAROL_SEC, 0; N, N, N; 0xB0);
        add_vote!(bstate, _c0_prime, CAROL, CAROL_SEC, 0; N, N, N; 0xB0);
        add_vote!(bstate, a0, ALICE, ALICE_SEC, 0; N, N, F; 0xA0);
        add_vote!(bstate, b0, BOB, BOB_SEC, 0; a0, N, F);
        add_vote!(bstate, a1, ALICE, ALICE_SEC, 1; a0, b0, F);
        assert_eq!(
            FinalityResult::Finalized(vec![0xA0], vec![CAROL]),
            fde4.run(&bstate)
        );
        Ok(())
    }
}
