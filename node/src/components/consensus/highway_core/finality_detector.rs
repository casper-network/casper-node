mod horizon;
mod rewards;

use std::iter;

use datasize::DataSize;
use tracing::{trace, warn};

use casper_types::Timestamp;

use crate::components::consensus::{
    consensus_protocol::{FinalizedBlock, TerminalBlockData},
    highway_core::{
        highway::Highway,
        state::{Observation, State, Unit, Weight},
        validators::ValidatorIndex,
    },
    traits::Context,
};
use horizon::Horizon;

/// An error returned if the configured fault tolerance has been exceeded.
#[derive(Debug)]
pub(crate) struct FttExceeded(pub Weight);

/// An incremental finality detector.
///
/// It reuses information between subsequent calls, so it must always be applied to the same
/// `State` instance: Later calls of `run` must see the same or a superset of the previous state.
#[derive(Debug, DataSize)]
pub(crate) struct FinalityDetector<C>
where
    C: Context,
{
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
    pub(crate) fn run<'a>(
        &'a mut self,
        highway: &'a Highway<C>,
    ) -> Result<impl Iterator<Item = FinalizedBlock<C>> + 'a, FttExceeded> {
        let state = highway.state();
        let fault_w = state.faulty_weight();
        if fault_w >= self.ftt || fault_w > (state.total_weight() - Weight(1)) / 2 {
            warn!(panorama = ?state.panorama(), "fault tolerance threshold exceeded");
            return Err(FttExceeded(fault_w));
        }
        Ok(iter::from_fn(move || {
            let bhash = self.next_finalized(state)?;
            // Safe to unwrap: Index exists, since we have units from them.
            let to_id = |vidx: ValidatorIndex| highway.validators().id(vidx).unwrap().clone();
            let block = state.block(bhash);
            let unit = state.unit(bhash);
            let terminal_block_data = state
                .is_terminal_block(bhash)
                .then(|| Self::create_terminal_block_data(bhash, unit, highway));
            let finalized_block = FinalizedBlock {
                value: block.value.clone(),
                timestamp: unit.timestamp,
                relative_height: block.height,
                terminal_block_data,
                equivocators: unit.panorama.iter_faulty().map(to_id).collect(),
                proposer: to_id(unit.creator),
            };
            trace!(panorama = ?state.panorama(), ?finalized_block, "finality detected");
            Some(finalized_block)
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
                self.last_finalized = Some(*candidate);
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
    pub(crate) fn find_summit(
        &self,
        target_lvl: usize,
        candidate: &C::Hash,
        state: &State<C>,
    ) -> usize {
        let total_w = state.total_weight();
        let quorum = self.quorum_for_lvl(target_lvl, total_w);
        let latest = state.panorama().iter().map(Observation::correct).collect();
        let sec0 = Horizon::level0(candidate, state, &latest);
        let horizons_iter = iter::successors(Some(sec0), |sec| sec.next(quorum));
        horizons_iter.skip(1).take(target_lvl).count()
    }

    /// Returns the quorum required by a summit with the specified level and the required FTT.
    #[allow(clippy::integer_arithmetic)] // See comments.
    fn quorum_for_lvl(&self, lvl: usize, total_w: Weight) -> Weight {
        // A level-lvl summit with quorum  total_w/2 + t  has relative FTT  2t(1 − 1/2^lvl). So:
        // quorum = total_w / 2 + ftt / 2 / (1 - 1/2^lvl)
        //        = total_w / 2 + 2^lvl * ftt / 2 / (2^lvl - 1)
        //        = ((2^lvl - 1) total_w + 2^lvl ftt) / (2 * 2^lvl - 2))
        // Levels higher than 63 have negligible effect. With 63, this can't overflow.
        let pow_lvl = 1u128 << lvl.min(63);
        // Since  pow_lvl <= 2^63,  we have  numerator < (2^64 - 1) * 2^64.
        // It is safe to subtract because  pow_lvl > 0.
        let numerator = (pow_lvl - 1) * u128::from(total_w) + pow_lvl * u128::from(self.ftt);
        // And  denominator < 2^64,  so  numerator + denominator < 2^128.
        let denominator = 2 * pow_lvl - 2;
        // The numerator is positive because  ftt > 0.
        // Since this is a lower bound for the quorum, we round up when dividing.
        Weight(((numerator + denominator - 1) / denominator) as u64)
    }

    /// Returns the next candidate for finalization, i.e. the lowest block in the fork choice that
    /// has not been finalized yet.
    fn next_candidate<'a>(&self, state: &'a State<C>) -> Option<&'a C::Hash> {
        let fork_choice = state.fork_choice(state.panorama())?;
        state.find_ancestor_proposal(fork_choice, self.next_height(state))
    }

    /// Returns the height of the next block that will be finalized.
    fn next_height(&self, state: &State<C>) -> u64 {
        // In a trillion years, we need to make block height u128.
        #[allow(clippy::integer_arithmetic)]
        let height_plus_1 = |bhash| state.block(bhash).height + 1;
        self.last_finalized.as_ref().map_or(0, height_plus_1)
    }

    /// Returns the hash of the last finalized block (if any).
    pub(crate) fn last_finalized(&self) -> Option<&C::Hash> {
        self.last_finalized.as_ref()
    }

    /// Returns the configured fault tolerance threshold of this detector.
    pub(crate) fn fault_tolerance_threshold(&self) -> Weight {
        self.ftt
    }

    /// Creates the information for the terminal block: which validators were inactive, and how
    /// rewards should be distributed.
    fn create_terminal_block_data(
        bhash: &C::Hash,
        unit: &Unit<C>,
        highway: &Highway<C>,
    ) -> TerminalBlockData<C> {
        // Safe to unwrap: Index exists, since we have units from them.
        let to_id = |vidx: ValidatorIndex| highway.validators().id(vidx).unwrap().clone();
        let state = highway.state();

        // Compute the rewards, and replace each validator index with the validator ID.
        let rewards = rewards::compute_rewards(state, bhash);
        let rewards_iter = rewards.enumerate();
        let rewards = rewards_iter.map(|(vidx, r)| (to_id(vidx), *r)).collect();

        // Report inactive validators, but only if they had sufficient time to create a unit, i.e.
        // if at least one maximum-length round passed between the first and last block.
        // Safe to unwrap: Ancestor at height 0 always exists.
        let first_bhash = state.find_ancestor_proposal(bhash, 0).unwrap();
        let sufficient_time_for_activity =
            unit.timestamp >= state.unit(first_bhash).timestamp + state.params().max_round_length();
        let inactive_validators = if sufficient_time_for_activity {
            unit.panorama.iter_none().map(to_id).collect()
        } else {
            Vec::new()
        };

        TerminalBlockData {
            rewards,
            inactive_validators,
        }
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
    fn finality_detector() -> Result<(), AddUnitError<TestContext>> {
        let mut state = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);

        // Create blocks with scores as follows:
        //
        //          a0: 9 — a1: 5
        //        /       \
        // b0: 10           b1: 4
        //        \
        //          c0: 1 — c1: 1
        let b0 = add_unit!(state, BOB, 0xB0; N, N, N)?;
        let c0 = add_unit!(state, CAROL, 0xC0; N, b0, N)?;
        let c1 = add_unit!(state, CAROL, 0xC1; N, b0, c0)?;
        let a0 = add_unit!(state, ALICE, 0xA0; N, b0, N)?;
        let a1 = add_unit!(state, ALICE, 0xA1; a0, b0, c1)?;
        let b1 = add_unit!(state, BOB, 0xB1; a0, b0, N)?;

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
        let _a2 = add_unit!(state, ALICE, None; a1, b1, c1)?;
        let _b2 = add_unit!(state, BOB, None; a1, b1, c1)?;
        assert_eq!(Some(&b0), fd6.next_finalized(&state));
        assert_eq!(None, fd6.next_finalized(&state));
        Ok(())
    }

    #[test]
    fn equivocators() -> Result<(), AddUnitError<TestContext>> {
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
        let b0 = add_unit!(state, BOB, 0xB0; N, N, N)?;
        let a0 = add_unit!(state, ALICE, 0xA0; N, b0, N)?;
        let c0 = add_unit!(state, CAROL, 0xC0; N, b0, N)?;
        let _c1 = add_unit!(state, CAROL, 0xC1; N, b0, c0)?;
        assert_eq!(Weight(0), state.faulty_weight());
        let _c1_prime = add_unit!(state, CAROL, None; N, b0, c0)?;
        assert_eq!(Weight(1), state.faulty_weight());
        let b1 = add_unit!(state, BOB, 0xB1; a0, b0, N)?;
        assert_eq!(Some(&b0), fd4.next_finalized(&state));
        let a1 = add_unit!(state, ALICE, 0xA1; a0, b0, F)?;
        let b2 = add_unit!(state, BOB, None; a1, b1, F)?;
        let a2 = add_unit!(state, ALICE, 0xA2; a1, b2, F)?;
        assert_eq!(Some(&a0), fd4.next_finalized(&state));
        assert_eq!(Some(&a1), fd4.next_finalized(&state));
        // Finalize A2.
        let b3 = add_unit!(state, BOB, None; a2, b2, F)?;
        let _a3 = add_unit!(state, ALICE, None; a2, b3, F)?;
        assert_eq!(Some(&a2), fd4.next_finalized(&state));

        // Test that an initial block reports equivocators as well.
        let mut bstate: State<TestContext> = State::new_test(&[Weight(5), Weight(4), Weight(1)], 0);
        let mut fde4 = FinalityDetector::new(Weight(4)); // Fault tolerance 4.
        let _c0 = add_unit!(bstate, CAROL, 0xC0; N, N, N)?;
        let _c0_prime = add_unit!(bstate, CAROL, 0xCC0; N, N, N)?;
        let a0 = add_unit!(bstate, ALICE, 0xA0; N, N, F)?;
        let b0 = add_unit!(bstate, BOB, None; a0, N, F)?;
        let _a1 = add_unit!(bstate, ALICE, None; a0, b0, F)?;
        assert_eq!(Some(&a0), fde4.next_finalized(&bstate));
        Ok(())
    }
}
