mod horizon;
mod rewards;

use std::iter;

use tracing::trace;

use crate::{
    components::consensus::{
        consensus_protocol::FinalizedBlock,
        highway_core::{
            highway::Highway,
            state::{Observation, State, Weight},
            validators::ValidatorIndex,
        },
        traits::Context,
        EraEnd,
    },
    types::Timestamp,
};
use horizon::Horizon;

/// An error returned if the configured fault tolerance has been exceeded.
#[derive(Debug)]
pub(crate) struct FttExceeded(Weight);

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
        assert!(ftt > Weight(0), "finality threshold must not be zero");
        FinalityDetector {
            last_finalized: None,
            ftt,
        }
    }

    /// Returns all blocks that have been finalized since the last call.
    // TODO: Verify the consensus instance ID?
    pub(crate) fn run<'a>(
        &'a mut self,
        highway: &'a Highway<C>,
    ) -> Result<
        impl Iterator<Item = FinalizedBlock<C::ConsensusValue, C::ValidatorId>> + 'a,
        FttExceeded,
    > {
        let state = highway.state();
        let fault_w = state.faulty_weight();
        if fault_w >= self.ftt || fault_w > (state.total_weight() - Weight(1)) / 2 {
            return Err(FttExceeded(fault_w));
        }
        Ok(iter::from_fn(move || {
            let bhash = self.next_finalized(state)?;
            let to_id = |vidx: ValidatorIndex| {
                let opt_validator = highway.validators().get_by_index(vidx);
                opt_validator.unwrap().id().clone() // Index exists, since we have votes from them.
            };
            let block = state.block(bhash);
            let vote = state.vote(bhash);
            let era_end = if state.is_terminal_block(bhash) {
                let rewards = rewards::compute_rewards(state, bhash);
                let rewards_iter = rewards.enumerate();
                let faulty_iter = vote.panorama.enumerate().filter(|(_, obs)| obs.is_faulty());
                Some(EraEnd {
                    equivocators: faulty_iter.map(|(vidx, _)| to_id(vidx)).collect(),
                    rewards: rewards_iter.map(|(vidx, r)| (to_id(vidx), *r)).collect(),
                })
            } else {
                None
            };

            Some(FinalizedBlock {
                value: block.value.clone(),
                timestamp: vote.timestamp,
                height: block.height,
                era_end,
                proposer: to_id(vote.creator),
            })
        }))
    }

    /// Returns the next block, if any has been finalized since the last call.
    pub(super) fn next_finalized<'a>(&mut self, state: &'a State<C>) -> Option<&'a C::Hash> {
        let start_time = Timestamp::now();
        let candidate = self.next_candidate(state)?;
        // For `lvl` → ∞, the quorum converges to a fixed value. After level 63, it is closer
        // to that limit than 1/2^-63. This won't make a difference in practice, so there is no
        // point looking for higher summits.
        let mut target_lvl = 63;
        while target_lvl > 0 {
            trace!(%target_lvl, "looking for summit");
            let lvl = self.find_summit(target_lvl, candidate, state);
            if lvl == target_lvl {
                self.last_finalized = Some(candidate.clone());
                let elapsed = start_time.elapsed();
                trace!(%elapsed, "found finalized block");
                return Some(candidate);
            }
            // The required quorum increases with decreasing level, so choosing `target_lvl`
            // greater than `lvl` would always yield a summit of level `lvl` or lower.
            target_lvl = lvl;
        }
        let elapsed = start_time.elapsed();
        trace!(%elapsed, "found no finalized block");
        None
    }

    /// Returns the number of levels of the highest summit with a quorum that a `target_lvl` summit
    /// would need for the desired FTT. If the returned number is `target_lvl` that means the
    /// `candidate` is finalized. If not, we need to retry with a lower `target_lvl`.
    fn find_summit(&self, target_lvl: usize, candidate: &C::Hash, state: &State<C>) -> usize {
        let total_w = state.total_weight();
        let quorum = self.quorum_for_lvl(target_lvl, total_w);
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
        assert!(lvl < 64, "lvl must be less than 64");
        let pow_lvl = 1u128 << lvl;
        // Since  pow_lvl <= 2^63,  we have  numerator < (2^64 - 1) * 2^64.
        let numerator = (pow_lvl - 1) * u128::from(total_w) + pow_lvl * u128::from(self.ftt);
        // And  denominator < 2^64,  so  numerator + denominator < 2^128.
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
    use crate::testing::TestRng;

    #[test]
    fn finality_detector() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut rng = TestRng::new();

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 5
        //        /       \
        // b0: 10           b1: 4
        //        \
        //          c0: 1 — c1: 1
        let b0 = add_vote!(state, rng, BOB, 0xB0; N, N, N)?;
        let c0 = add_vote!(state, rng, CAROL, 0xC0; N, b0, N)?;
        let c1 = add_vote!(state, rng, CAROL, 0xC1; N, b0, c0)?;
        let a0 = add_vote!(state, rng, ALICE, 0xA0; N, b0, N)?;
        let a1 = add_vote!(state, rng, ALICE, 0xA1; a0, b0, c1)?;
        let b1 = add_vote!(state, rng, BOB, 0xB1; a0, b0, N)?;

        let mut fd4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        let mut fd6 = FinalityDetector::new(Weight(6)); // Fault tolerance 6.

        // The total weight of our validators is 10.
        // A level-k summit with quorum q has relative FTT  2 (q - 10/2) (1 − 1/2^k).
        //
        // `b0`, `a0` are level 0 for `B0`. `a0`, `b1` are level 1.
        // So the fault tolerance of `B0` is 2 * (9 - 10/2) * (1 - 1/2) = 4.
        assert_eq!(None, fd6.next_finalized(&state));
        assert_eq!(Some(&b0), fd4.next_finalized(&state));
        assert_eq!(None, fd4.next_finalized(&state));

        // Adding another level to the summit increases `B0`'s fault tolerance to 6.
        let _a2 = add_vote!(state, rng, ALICE, None; a1, b1, c1)?;
        let _b2 = add_vote!(state, rng, BOB, None; a1, b1, c1)?;
        assert_eq!(Some(&b0), fd6.next_finalized(&state));
        assert_eq!(None, fd6.next_finalized(&state));
        Ok(())
    }

    #[test]
    fn equivocators() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fd4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        let mut rng = TestRng::new();

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 9 - a2: 5 - a3: 5
        //        /       \      \       \
        // b0: 10           b1: 4  b2: 4  b3: 4
        //        \
        //          c0: 1 — c1: 1
        //               \ c1': 1
        let b0 = add_vote!(state, rng, BOB, 0xB0; N, N, N)?;
        let a0 = add_vote!(state, rng, ALICE, 0xA0; N, b0, N)?;
        let c0 = add_vote!(state, rng, CAROL, 0xC0; N, b0, N)?;
        let _c1 = add_vote!(state, rng, CAROL, 0xC1; N, b0, c0)?;
        assert_eq!(Weight(0), state.faulty_weight());
        let _c1_prime = add_vote!(state, rng, CAROL, None; N, b0, c0)?;
        assert_eq!(Weight(1), state.faulty_weight());
        let b1 = add_vote!(state, rng, BOB, 0xB1; a0, b0, N)?;
        assert_eq!(Some(&b0), fd4.next_finalized(&state));
        let a1 = add_vote!(state, rng, ALICE, 0xA1; a0, b0, F)?;
        let b2 = add_vote!(state, rng, BOB, None; a1, b1, F)?;
        let a2 = add_vote!(state, rng, ALICE, 0xA2; a1, b2, F)?;
        assert_eq!(Some(&a0), fd4.next_finalized(&state));
        assert_eq!(Some(&a1), fd4.next_finalized(&state));
        // Finalize A2.
        let b3 = add_vote!(state, rng, BOB, None; a2, b2, F)?;
        let _a3 = add_vote!(state, rng, ALICE, None; a2, b3, F)?;
        assert_eq!(Some(&a2), fd4.next_finalized(&state));

        // Test that an initial block reports equivocators as well.
        let mut bstate: State<TestContext> = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fde4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        let _c0 = add_vote!(bstate, rng, CAROL, 0xB0; N, N, N)?;
        let _c0_prime = add_vote!(bstate, rng, CAROL, 0xB0; N, N, N)?;
        let a0 = add_vote!(bstate, rng, ALICE, 0xA0; N, N, F)?;
        let b0 = add_vote!(bstate, rng, BOB, None; a0, N, F)?;
        let _a1 = add_vote!(bstate, rng, ALICE, None; a0, b0, F)?;
        assert_eq!(Some(&a0), fde4.next_finalized(&bstate));
        Ok(())
    }
}
