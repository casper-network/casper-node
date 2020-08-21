mod horizon;
mod rewards;

use std::{collections::BTreeMap, iter};

use super::{
    highway::Highway,
    state::{Observation, State, Weight},
    validators::ValidatorIndex,
};
use crate::{components::consensus::traits::Context, types::Timestamp};
use horizon::Horizon;

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
        /// Rewards for finalization of earlier blocks.
        ///
        /// This is a measure of the value of each validator's contribution to consensus, in
        /// fractions of a maximum `BLOCK_REWARD`.
        rewards: BTreeMap<C::ValidatorId, u64>,
        /// The timestamp at which this value was proposed.
        timestamp: Timestamp,
        /// The relative height in this instance of the protocol.
        height: u64,
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
        let to_id = |vidx: ValidatorIndex| highway.validators().get_by_index(vidx).id().clone();
        let new_equivocators_iter = state.get_new_equivocators(bhash).into_iter();
        let rewards = rewards::compute_rewards(state, bhash);
        let rewards_iter = rewards.enumerate();
        let block = state.block(bhash);
        FinalityOutcome::Finalized {
            value: block.value.clone(),
            new_equivocators: new_equivocators_iter.map(to_id).collect(),
            rewards: rewards_iter.map(|(vidx, r)| (to_id(vidx), *r)).collect(),
            timestamp: state.vote(bhash).timestamp,
            height: block.height,
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
        let latest = state.panorama().iter().map(Observation::correct).collect();
        let sec0 = Horizon::level0(candidate, &state, &latest);
        let horizons_iter = iter::successors(Some(sec0), |sec| sec.next(quorum));
        horizons_iter.skip(1).take(target_lvl).count()
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

#[allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.
#[cfg(test)]
mod tests {
    use super::{
        super::state::{tests::*, State},
        *,
    };

    #[test]
    fn finality_detector() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 5
        //        /       \
        // b0: 10           b1: 4
        //        \
        //          c0: 1 — c1: 1
        let b0 = add_vote!(state, BOB, 0xB0; N, N, N)?;
        let c0 = add_vote!(state, CAROL, 0xC0; N, b0, N)?;
        let c1 = add_vote!(state, CAROL, 0xC1; N, b0, c0)?;
        let a0 = add_vote!(state, ALICE, 0xA0; N, b0, N)?;
        let a1 = add_vote!(state, ALICE, 0xA1; a0, b0, c1)?;
        let b1 = add_vote!(state, BOB, 0xB1; a0, b0, N)?;

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
        let _a2 = add_vote!(state, ALICE, None; a1, b1, c1)?;
        let _b2 = add_vote!(state, BOB, None; a1, b1, c1)?;
        assert_eq!(Some(&b0), fd6.next_finalized(&state, 0.into()));
        assert_eq!(None, fd6.next_finalized(&state, 0.into()));

        // If Bob equivocates, the FTT 4 is exceeded, but she counts as being part of any summit,
        // so `A0` and `A1` get FTT 6. (Bob voted for `A1` and against `B1` in `b2`.)
        assert_eq!(Weight(0), state.faulty_weight());
        let _e2 = add_vote!(state, BOB, None; a1, b1, c1)?;
        assert_eq!(Weight(4), state.faulty_weight());
        assert_eq!(Some(&a0), fd6.next_finalized(&state, 4.into()));
        assert_eq!(Some(&a1), fd6.next_finalized(&state, 4.into()));
        assert_eq!(None, fd6.next_finalized(&state, 4.into()));
        Ok(())
    }

    #[test]
    fn equivocators() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fd4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 9 - a2: 5 - a3: 5
        //        /       \      \       \
        // b0: 10           b1: 4  b2: 4  b3: 4
        //        \
        //          c0: 1 — c1: 1
        //               \ c1': 1
        let b0 = add_vote!(state, BOB, 0xB0; N, N, N)?;
        let a0 = add_vote!(state, ALICE, 0xA0; N, b0, N)?;
        let c0 = add_vote!(state, CAROL, 0xC0; N, b0, N)?;
        let _c1 = add_vote!(state, CAROL, 0xC1; N, b0, c0)?;
        assert_eq!(Weight(0), state.faulty_weight());
        let _c1_prime = add_vote!(state, CAROL, None; N, b0, c0)?;
        assert_eq!(Weight(1), state.faulty_weight());
        let b1 = add_vote!(state, BOB, 0xB1; a0, b0, N)?;
        assert_eq!(Some(&b0), fd4.next_finalized(&state, 1.into()));
        let a1 = add_vote!(state, ALICE, 0xA1; a0, b0, F)?;
        let b2 = add_vote!(state, BOB, None; a1, b1, F)?;
        let a2 = add_vote!(state, ALICE, 0xA2; a1, b2, F)?;
        assert_eq!(Some(&a0), fd4.next_finalized(&state, 1.into()));
        // A1 is the first block that sees CAROL equivocating.
        assert_eq!(vec![CAROL], state.get_new_equivocators(&a1));
        assert_eq!(Some(&a1), fd4.next_finalized(&state, 1.into()));
        // Finalize A2. It should not report CAROL as equivocator anymore.
        let b3 = add_vote!(state, BOB, None; a2, b2, F)?;
        let _a3 = add_vote!(state, ALICE, None; a2, b3, F)?;
        assert!(state.get_new_equivocators(&a2).is_empty());
        assert_eq!(Some(&a2), fd4.next_finalized(&state, 1.into()));

        // Test that an initial block reports equivocators as well.
        let mut bstate: State<TestContext> = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fde4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        let _c0 = add_vote!(bstate, CAROL, 0xB0; N, N, N)?;
        let _c0_prime = add_vote!(bstate, CAROL, 0xB0; N, N, N)?;
        let a0 = add_vote!(bstate, ALICE, 0xA0; N, N, F)?;
        let b0 = add_vote!(bstate, BOB, None; a0, N, F)?;
        let _a1 = add_vote!(bstate, ALICE, None; a0, b0, F)?;
        assert_eq!(vec![CAROL], bstate.get_new_equivocators(&a0));
        assert_eq!(Some(&a0), fde4.next_finalized(&bstate, 1.into()));
        Ok(())
    }
}
