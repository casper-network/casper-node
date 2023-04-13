use tracing::trace;

use casper_types::Timestamp;

use super::Horizon;
use crate::components::consensus::{
    highway_core::{
        state::{Observation, Panorama, State, Weight},
        validators::ValidatorMap,
    },
    traits::Context,
};

/// Returns the map of rewards to be paid out when the block `bhash` gets finalized.
///
/// This is the sum of all rewards for finalization of ancestors of `bhash`, as seen from `bhash`.
pub(crate) fn compute_rewards<C: Context>(state: &State<C>, bhash: &C::Hash) -> ValidatorMap<u64> {
    // The unit that introduced the payout block.
    let payout_unit = state.unit(bhash);
    // The panorama of the payout block: Rewards must only use this panorama, since it defines
    // what everyone who has the block can already see.
    let panorama = &payout_unit.panorama;
    let mut rewards = ValidatorMap::from(vec![0u64; panorama.len()]);
    for proposal_hash in state.ancestor_hashes(bhash) {
        for (vidx, r) in compute_rewards_for(state, panorama, proposal_hash).enumerate() {
            match rewards[vidx].checked_add(*r) {
                Some(sum) => rewards[vidx] = sum,
                // Rewards should not overflow. We use one trillion for a block reward, so the full
                // rewards for 18 million blocks fit into a u64.
                None => panic!(
                    "rewards for {:?}, {} + {}, overflow u64",
                    vidx, rewards[vidx], r
                ),
            }
        }
    }
    rewards
}

/// Returns the rewards for finalizing the block with hash `proposal_h`.
fn compute_rewards_for<C: Context>(
    state: &State<C>,
    panorama: &Panorama<C>,
    proposal_h: &C::Hash,
) -> ValidatorMap<u64> {
    let proposal_unit = state.unit(proposal_h);
    let r_id = proposal_unit.round_id();

    // Only consider messages in round `r_id` for the summit. To compute the assigned weight, we
    // also include validators who didn't send a message in that round, but were supposed to.
    let mut assigned_weight = Weight(0);
    let mut latest = ValidatorMap::from(vec![None; panorama.len()]);
    for (idx, obs) in panorama.enumerate() {
        match round_participation(state, obs, r_id) {
            RoundParticipation::Unassigned => continue,
            RoundParticipation::No => (),
            RoundParticipation::Yes(latest_vh) => latest[idx] = Some(latest_vh),
        }
        assigned_weight += state.weight(idx);
    }

    if assigned_weight.is_zero() {
        return ValidatorMap::from(vec![0; latest.len()]);
    }

    // Find all level-1 summits. For each validator, store the highest quorum it is a part of.
    let horizon = Horizon::level0(proposal_h, state, &latest);
    let (mut committee, _) = horizon.prune_committee(Weight(1), latest.keys_some().collect());
    let mut max_quorum = ValidatorMap::from(vec![Weight(0); latest.len()]);
    while let Some(quorum) = horizon.committee_quorum(&committee) {
        // The current committee is a level-1 summit with `quorum`. Try to go higher:
        let (new_committee, pruned) = horizon.prune_committee(quorum + Weight(1), committee);
        committee = new_committee;
        // Pruned validators are not part of any summit with a higher quorum than this.
        for vidx in pruned {
            max_quorum[vidx] = quorum;
        }
    }

    let faulty_w: Weight = panorama.iter_faulty().map(|vidx| state.weight(vidx)).sum();

    // Collect the block rewards for each validator who is a member of at least one summit.
    #[allow(clippy::integer_arithmetic)] // See inline comments.
    max_quorum
        .enumerate()
        .zip(state.weights())
        .map(|((validator_index, quorum), weight)| {
            // If the summit's quorum was not enough to finalize the block, rewards are reduced.
            // A level-1 summit with quorum  q  has FTT  q - 50%, so we need  q - 50% > f.
            let finality_factor = if *quorum > (state.total_weight() / 2).saturating_add(faulty_w) {
                state.params().block_reward()
            } else {
                state.params().reduced_block_reward()
            };
            trace!(
                validator_index = validator_index.0,
                finality_factor,
                quorum = quorum.0,
                assigned_weight = assigned_weight.0,
                weight = weight.0,
                timestamp = %proposal_unit.timestamp,
                "block reward calculation"
            );
            // Rewards are proportional to the quorum and to the validator's weight.
            // Since  quorum <= assigned_weight  and  weight <= total_weight,  this won't overflow.
            (u128::from(finality_factor) * u128::from(*quorum) / u128::from(assigned_weight)
                * u128::from(*weight)
                / u128::from(state.total_weight())) as u64
        })
        .collect()
}

/// Information about how a validator participated in a particular round.
#[derive(Debug, PartialEq)]
enum RoundParticipation<'a, C: Context> {
    /// The validator was not assigned: The round ID was not the beginning of one of their rounds.
    Unassigned,
    /// The validator was assigned but did not create any messages in that round.
    No,
    /// The validator participated, and this is their latest message in that round.
    Yes(&'a C::Hash),
}

/// Returns information about the participation of a validator with `obs` in round `r_id`.
fn round_participation<'a, C: Context>(
    state: &'a State<C>,
    obs: &'a Observation<C>,
    r_id: Timestamp,
) -> RoundParticipation<'a, C> {
    // Find the validator's latest unit in or before round `r_id`.
    let maybe_unit = match obs {
        Observation::Faulty => return RoundParticipation::Unassigned,
        Observation::None => return RoundParticipation::No,
        Observation::Correct(latest_vh) => state
            .swimlane(latest_vh)
            .find(|&(_, unit)| unit.round_id() <= r_id),
    };
    maybe_unit.map_or(RoundParticipation::No, |(vh, unit)| {
        if unit.round_exp > r_id.trailing_zeros() {
            // Round length doesn't divide `r_id`, so the validator was not assigned to that round.
            RoundParticipation::Unassigned
        } else if unit.timestamp < r_id {
            // The latest unit in or before `r_id` was before `r_id`, so they didn't participate.
            RoundParticipation::No
        } else {
            RoundParticipation::Yes(vh)
        }
    })
}

#[allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.
#[allow(clippy::integer_arithmetic)] // Overflows in tests would panic anyway.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::consensus::highway_core::{
        highway_testing::{TEST_BLOCK_REWARD, TEST_ENDORSEMENT_EVIDENCE_LIMIT},
        state::{tests::*, Params},
        validators::ValidatorMap,
    };

    #[test]
    fn round_participation_test() -> Result<(), AddUnitError<TestContext>> {
        let mut state = State::new_test(&[Weight(5)], 0); // Alice is the only validator.

        // Round ID 0, length 32: Alice participates.
        let p0 = add_unit!(state, ALICE, 0, 5u8, 0x1; N)?; // Proposal
        let w0 = add_unit!(state, ALICE, 20, 5u8, None; p0)?; // Witness

        // Round ID 32, length 32: Alice partially participates.
        let w32 = add_unit!(state, ALICE, 52, 5u8, None; w0)?; // Witness

        // Round ID 64, length 32: Alice doesn't participate.

        // Round ID 96, length 16: Alice participates.
        let p96 = add_unit!(state, ALICE, 96, 4u8, 0x2; w32)?;
        let w96 = add_unit!(state, ALICE, 106, 4u8, None; p96)?;

        let obs = Observation::Correct(w96);
        let rp = |time: u64| round_participation(&state, &obs, Timestamp::from(time));
        assert_eq!(RoundParticipation::Yes(&w0), rp(0));
        assert_eq!(RoundParticipation::Unassigned, rp(16));
        assert_eq!(RoundParticipation::Yes(&w32), rp(32));
        assert_eq!(RoundParticipation::No, rp(64));
        assert_eq!(RoundParticipation::Unassigned, rp(80));
        assert_eq!(RoundParticipation::Yes(&w96), rp(96));
        assert_eq!(RoundParticipation::Unassigned, rp(106));
        assert_eq!(RoundParticipation::No, rp(112));
        Ok(())
    }

    // To keep the form of the reward formula, we spell out Carol's weight 1.
    #[allow(clippy::identity_op)]
    #[test]
    fn compute_rewards_test() -> Result<(), AddUnitError<TestContext>> {
        const ALICE_W: u64 = 4;
        const BOB_W: u64 = 5;
        const CAROL_W: u64 = 1;
        const ALICE_BOB_W: u64 = ALICE_W + BOB_W;
        const ALICE_CAROL_W: u64 = ALICE_W + CAROL_W;
        const BOB_CAROL_W: u64 = BOB_W + CAROL_W;

        let params = Params::new(
            0,
            TEST_BLOCK_REWARD,
            TEST_BLOCK_REWARD / 5,
            3,
            19,
            3,
            u64::MAX,
            Timestamp::zero(),
            Timestamp::from(u64::MAX),
            TEST_ENDORSEMENT_EVIDENCE_LIMIT,
        );
        let weights = &[Weight(ALICE_W), Weight(BOB_W), Weight(CAROL_W)];
        let mut state = State::new(weights, params, vec![], vec![]);
        let total_weight = state.total_weight().0;

        // Round 0: Alice has round length 16, Bob and Carol 8.
        // Bob and Alice cite each other, creating a summit with quorum 9.
        // Carol only cites Bob, so she's only part of a quorum-6 summit.
        assert_eq!(BOB, state.leader(0.into()));
        let bp0 = add_unit!(state, BOB, 0, 3u8, 0xB00; N, N, N)?;
        let ac0 = add_unit!(state, ALICE, 1, 4u8, None; N, bp0, N)?;
        let cc0 = add_unit!(state, CAROL, 1, 3u8, None; N, bp0, N)?;
        let bw0 = add_unit!(state, BOB, 5, 3u8, None; ac0, bp0, cc0)?;
        let cw0 = add_unit!(state, CAROL, 5, 3u8, None; N, bp0, cc0)?;
        let aw0 = add_unit!(state, ALICE, 10, 4u8, None; ac0, bp0, N)?;

        let assigned = total_weight; // Everyone is assigned to round 0.
        let rewards0 = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * BOB_CAROL_W * CAROL_W / (assigned * total_weight),
        ]);

        // Round 8: Alice is not assigned (length 16). Bob and Carol make a summit.
        assert_eq!(BOB, state.leader(8.into()));
        let bp8 = add_unit!(state, BOB, 8, 3u8, 0xB08; ac0, bw0, cw0)?;
        let cc8 = add_unit!(state, CAROL, 9, 3u8, None; ac0, bp8, cw0)?;
        let bw8 = add_unit!(state, BOB, 13, 3u8, None; aw0, bp8, cc8)?;
        let cw8 = add_unit!(state, CAROL, 13, 3u8, None; aw0, bp8, cc8)?;

        let assigned = BOB_CAROL_W; // Alice has round length 16, so she's not in round 8.
        let rewards8 = ValidatorMap::from(vec![
            0,
            TEST_BLOCK_REWARD * BOB_CAROL_W * BOB_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * BOB_CAROL_W * CAROL_W / (assigned * total_weight),
        ]);

        // Round 16: Carol slows down (length 16). Alice and Bob finalize with quorum 9.
        // Carol cites only Alice and herself, so she's only in the non-finalizing quorum-5 summit.
        assert_eq!(ALICE, state.leader(16.into()));
        let ap16 = add_unit!(state, ALICE, 16, 4u8, 0xA16; aw0, bw8, cw8)?;
        let bc16 = add_unit!(state, BOB, 17, 3u8, None; ap16, bw8, cw8)?;
        let cc16 = add_unit!(state, CAROL, 17, 4u8, None; ap16, bw8, cw8)?;
        let bw16 = add_unit!(state, BOB, 19, 3u8, None; ap16, bc16, cw8)?;
        let aw16 = add_unit!(state, ALICE, 26, 4u8, None; ap16, bc16, cc16)?;
        let cw16 = add_unit!(state, CAROL, 26, 4u8, None; ap16, bw8, cc16)?;

        let assigned = total_weight; // Everyone is assigned.
        let reduced_reward = state.params().reduced_block_reward();
        let rewards16 = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            reduced_reward * ALICE_CAROL_W * CAROL_W / (assigned * total_weight),
        ]);

        // Produce a block that can see all three rounds but doesn't see Carol equivocate.
        let ap_last = add_unit!(state, ALICE, 0x0; aw16, bw16, cw16)?;

        // Alice's round-16 block can see the first two rounds.
        let expected = ValidatorMap::from(vec![
            rewards0[ALICE] + rewards8[ALICE],
            rewards0[BOB] + rewards8[BOB],
            rewards0[CAROL] + rewards8[CAROL],
        ]);
        assert_eq!(expected, compute_rewards(&state, &ap16));

        // Her next block can also see round 16.
        let expected = ValidatorMap::from(vec![
            rewards0[ALICE] + rewards8[ALICE] + rewards16[ALICE],
            rewards0[BOB] + rewards8[BOB] + rewards16[BOB],
            rewards0[CAROL] + rewards8[CAROL] + rewards16[CAROL],
        ]);
        let pan = &state.unit(&ap_last).panorama;
        assert_eq!(rewards0, compute_rewards_for(&state, pan, &bp0));
        assert_eq!(rewards8, compute_rewards_for(&state, pan, &bp8));
        assert_eq!(rewards16, compute_rewards_for(&state, pan, &ap16));
        assert_eq!(expected, compute_rewards(&state, &ap_last));

        // However, Carol also equivocated in round 16. And Bob saw her!
        let _cw16e = add_unit!(state, CAROL, 26, 4u8, None; ap16, bc16, cc16)?;
        let bp_last = add_unit!(state, ALICE, 0x0; aw16, bw16, F)?;

        let assigned = ALICE_BOB_W; // Carol is unassigned if she is seen as faulty.
        let rewards0f = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            0,
        ]);

        // Bob alone has only 50% of the weight. Not enough to finalize, so only reduced reward.
        let assigned = BOB_W; // Alice has round length 16 and Carol is faulty.
        let rewards8f = ValidatorMap::from(vec![
            0,
            reduced_reward * BOB_W * BOB_W / (assigned * total_weight),
            0,
        ]);

        let assigned = ALICE_BOB_W; // Carol is unassigned if she is seen as faulty.
        let rewards16f = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            0,
        ]);

        // Bob's block sees Carol as faulty.
        let expected = ValidatorMap::from(vec![
            rewards0f[ALICE] + rewards8f[ALICE] + rewards16f[ALICE],
            rewards0f[BOB] + rewards8f[BOB] + rewards16f[BOB],
            rewards0f[CAROL] + rewards8f[CAROL] + rewards16f[CAROL],
        ]);
        let pan = &state.unit(&bp_last).panorama;
        assert_eq!(rewards0f, compute_rewards_for(&state, pan, &bp0));
        assert_eq!(rewards8f, compute_rewards_for(&state, pan, &bp8));
        assert_eq!(rewards16f, compute_rewards_for(&state, pan, &ap16));
        assert_eq!(expected, compute_rewards(&state, &bp_last));

        Ok(())
    }
}
