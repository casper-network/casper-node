use itertools::Itertools;
use tracing::error;

use super::Horizon;
use crate::{
    components::consensus::{
        highway_core::{
            state::{Observation, Panorama, State, Weight},
            validators::ValidatorMap,
        },
        traits::Context,
    },
    types::Timestamp,
};

/// Returns the map of rewards to be paid out when the block `bhash` gets finalized.
pub(crate) fn compute_rewards<C: Context>(state: &State<C>, bhash: &C::Hash) -> ValidatorMap<u64> {
    // The newly finalized block, in which the rewards are paid out.
    let payout_block = state.block(bhash);
    // The vote that introduced the payout block.
    let payout_vote = state.vote(bhash);
    // The panorama of the payout block: Rewards must only use this panorama, since it defines
    // what everyone who has the block can already see.
    let panorama = &payout_vote.panorama;
    // The vote that introduced the payout block's parent.
    let opt_parent_vote = payout_block.parent().map(|h| state.vote(h));
    // The parent's timestamp, or 0.
    let parent_time = opt_parent_vote.map_or_else(Timestamp::zero, |vote| vote.timestamp);
    // Only summits for blocks for which rewards are scheduled between the previous and current
    // timestamps are rewarded, and only if they are ancestors of the payout block.
    let range = parent_time..payout_vote.timestamp;
    let is_ancestor =
        |bh: &&C::Hash| Some(*bh) == state.find_ancestor(bhash, state.block(bh).height);
    let mut rewards = ValidatorMap::from(vec![0u64; panorama.len()]);
    for bhash in state.rewards_range(range).filter(is_ancestor).unique() {
        for (vidx, r) in compute_rewards_for(state, panorama, bhash).enumerate() {
            match rewards[vidx].checked_add(*r) {
                Some(sum) => rewards[vidx] = sum,
                None => {
                    error!(
                        "rewards for {:?}, {} + {}, saturate u64",
                        vidx, rewards[vidx], r
                    );
                    rewards[vidx] = u64::MAX;
                }
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
    let fault_w = state.faulty_weight_in(panorama);
    let proposal_vote = state.vote(proposal_h);
    let r_id = proposal_vote.round_id();

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

    // Collect the block rewards for each validator who is a member of at least one summit.
    max_quorum
        .iter()
        .zip(state.weights())
        .map(|(quorum, weight)| {
            // If the summit's quorum was not enough to finalize the block, rewards are reduced.
            let finality_factor = if *quorum > state.total_weight() / 2 + fault_w {
                state.params().block_reward()
            } else {
                state.params().reduced_block_reward()
            };
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
    // Find the validator's latest vote in or before round `r_id`.
    let opt_vote = match obs {
        Observation::Faulty => return RoundParticipation::Unassigned,
        Observation::None => return RoundParticipation::No,
        Observation::Correct(latest_vh) => state
            .swimlane(latest_vh)
            .find(|&(_, vote)| vote.round_id() <= r_id),
    };
    opt_vote.map_or(RoundParticipation::No, |(vh, vote)| {
        if vote.round_exp > r_id.trailing_zeros() {
            // Round length doesn't divide `r_id`, so the validator was not assigned to that round.
            RoundParticipation::Unassigned
        } else if vote.timestamp < r_id {
            // The latest vote in or before `r_id` was before `r_id`, so they didn't participate.
            RoundParticipation::No
        } else {
            RoundParticipation::Yes(vh)
        }
    })
}

#[allow(unused_qualifications)] // This is to suppress warnings originating in the test macros.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        components::consensus::highway_core::{
            highway_testing::{TEST_BLOCK_REWARD, TEST_REWARD_DELAY},
            state::tests::*,
            validators::ValidatorMap,
        },
        testing::TestRng,
    };

    #[test]
    fn round_participation_test() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::new_test(&[Weight(5)], 0); // Alice is the only validator.
        let mut rng = TestRng::new();
        let instance_id = 1u64;

        // Round ID 0, length 16: Alice participates.
        let p0 = add_vote!(state, instance_id, rng, ALICE, 0, 4u8, 0x1; N)?; // Proposal
        let w0 = add_vote!(state, instance_id, rng, ALICE, 10, 4u8, None; p0)?; // Witness

        // Round ID 16, length 16: Alice partially participates.
        let w16 = add_vote!(state, instance_id, rng, ALICE, 26, 4u8, None; w0)?; // Witness

        // Round ID 32, length 16: Alice doesn't participate.

        // Round ID 48, length 8: Alice participates.
        let p48 = add_vote!(state, instance_id, rng, ALICE, 48, 3u8, 0x2; w16)?;
        let w48 = add_vote!(state, instance_id, rng, ALICE, 53, 3u8, None; p48)?;

        let obs = Observation::Correct(w48);
        let rp = |time: u64| round_participation(&state, &obs, Timestamp::from(time));
        assert_eq!(RoundParticipation::Yes(&w0), rp(0));
        assert_eq!(RoundParticipation::Unassigned, rp(8));
        assert_eq!(RoundParticipation::Yes(&w16), rp(16));
        assert_eq!(RoundParticipation::No, rp(32));
        assert_eq!(RoundParticipation::Unassigned, rp(40));
        assert_eq!(RoundParticipation::Yes(&w48), rp(48));
        assert_eq!(RoundParticipation::Unassigned, rp(52));
        assert_eq!(RoundParticipation::No, rp(56));
        Ok(())
    }

    // To keep the form of the reward formula, we spell out Carol's weight 1.
    #[allow(clippy::identity_op)]
    #[test]
    fn compute_rewards_test() -> Result<(), AddVoteError<TestContext>> {
        const ALICE_W: u64 = 4;
        const BOB_W: u64 = 5;
        const CAROL_W: u64 = 1;
        const ALICE_BOB_W: u64 = ALICE_W + BOB_W;
        const ALICE_CAROL_W: u64 = ALICE_W + CAROL_W;
        const BOB_CAROL_W: u64 = BOB_W + CAROL_W;

        let mut state = State::new_test(&[Weight(ALICE_W), Weight(BOB_W), Weight(CAROL_W)], 0);
        let total_weight = state.total_weight().0;
        let mut rng = TestRng::new();
        let instance_id = 1u64;

        // Payouts for a block happen at the first occasion after `TEST_REWARD_DELAY` times the
        // block's round length.
        let payday0 = 0 + (TEST_REWARD_DELAY + 1) * 8;
        let payday8 = 8 + (TEST_REWARD_DELAY + 1) * 8;
        let payday16 = 16 + (TEST_REWARD_DELAY + 1) * 16; // Alice had round length 16.
        let pre16 = payday16 - 16;

        // Round 0: Alice has round length 16, Bob and Carol 8.
        // Bob and Alice cite each other, creating a summit with quorum 9.
        // Carol only cites Bob, so she's only part of a quorum-6 summit.
        assert_eq!(BOB, state.leader(0.into()));
        let bp0 = add_vote!(state, instance_id, rng, BOB, 0, 3u8, 0xB00; N, N, N)?;
        let ac0 = add_vote!(state, instance_id, rng, ALICE, 1, 4u8, None; N, bp0, N)?;
        let cc0 = add_vote!(state, instance_id, rng, CAROL, 1, 3u8, None; N, bp0, N)?;
        let bw0 = add_vote!(state, instance_id, rng, BOB, 5, 3u8, None; ac0, bp0, cc0)?;
        let cw0 = add_vote!(state, instance_id, rng, CAROL, 5, 3u8, None; N, bp0, cc0)?;
        let aw0 = add_vote!(state, instance_id, rng, ALICE, 10, 4u8, None; ac0, bp0, N)?;

        // Round 8: Alice is not assigned (length 16). Bob and Carol make a summit.
        assert_eq!(BOB, state.leader(8.into()));
        let bp8 = add_vote!(state, instance_id, rng, BOB, 8, 3u8, 0xB08; ac0, bw0, cw0)?;
        let cc8 = add_vote!(state, instance_id, rng, CAROL, 9, 3u8, None; ac0, bp8, cw0)?;
        let bw8 = add_vote!(state, instance_id, rng, BOB, 13, 3u8, None; aw0, bp8, cc8)?;
        let cw8 = add_vote!(state, instance_id, rng, CAROL, 13, 3u8, None; aw0, bp8, cc8)?;

        // Round 16: Carol slows down (length 16). Alice and Bob finalize with quorum 9.
        // Carol cites only Alice and herself, so she's only in the non-finalizing quorum-5 summit.
        assert_eq!(ALICE, state.leader(16.into()));
        let ap16 = add_vote!(state, instance_id, rng, ALICE, 16, 4u8, 0xA16; aw0, bw8, cw8)?;
        let bc16 = add_vote!(state, instance_id, rng, BOB, 17, 3u8, None; ap16, bw8, cw8)?;
        let cc16 = add_vote!(state, instance_id, rng, CAROL, 17, 4u8, None; ap16, bw8, cw8)?;
        let bw16 = add_vote!(state, instance_id, rng, BOB, 19, 3u8, None; ap16, bc16, cw8)?;
        let aw16 = add_vote!(state, instance_id, rng, ALICE, 26, 4u8, None; ap16, bc16, cc16)?;
        let cw16 = add_vote!(state, instance_id, rng, CAROL, 26, 4u8, None; ap16, bw8, cc16)?;

        // Produce blocks where rewards for rounds 0 and 8 are paid out.
        assert_eq!(ALICE, state.leader(payday0.into()));
        let pay0 = add_vote!(state, instance_id, rng, ALICE, payday0, 3u8, 0x0; aw16, bw16, cw16)?;
        assert_eq!(BOB, state.leader(payday8.into()));
        let pay8 = add_vote!(state, instance_id, rng, BOB, payday8, 3u8, 0x8; pay0, bw16, cw16)?;

        // Produce another (possibly equivocating) block where rewards for 0 and 8 are paid out,
        // and then one where the reward for round 16 is paid. This is to avoid adding rewards for
        // `pay0` and `pay8` themselves.
        assert_eq!(ALICE, state.leader(pre16.into()));
        let pay_pre16 =
            add_vote!(state, instance_id, rng, ALICE, pre16, 3u8, 0x0; aw16, bw16, cw16)?;
        assert_eq!(ALICE, state.leader(payday16.into()));
        let pay16 =
            add_vote!(state, instance_id, rng, ALICE, payday16, 3u8, 0x0; pay_pre16, bw16, cw16)?;

        // Finally create another block that saw Carol equivocate in round 16.
        let _cw16e = add_vote!(state, instance_id, rng, CAROL, 26, 4u8, None; ap16, bc16, cc16)?;
        let pay16f =
            add_vote!(state, instance_id, rng, ALICE, payday16, 3u8, 0x0; pay_pre16, bw16, F)?;

        // Round 0: Alice and Bob have quorum 9, Carol 6.
        let assigned = total_weight;
        let expected0 = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * BOB_CAROL_W * CAROL_W / (assigned * total_weight),
        ]);
        assert_eq!(expected0, compute_rewards(&state, &pay0));

        // Round 8: Only Bob and Carol were assigned, and finalized the round's block with quorum 6.
        let assigned = BOB_CAROL_W;
        let expected8 = ValidatorMap::from(vec![
            0,
            TEST_BLOCK_REWARD * BOB_CAROL_W * BOB_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * BOB_CAROL_W * CAROL_W / (assigned * total_weight),
        ]);
        assert_eq!(expected8, compute_rewards(&state, &pay8));

        // Before round 16: Rewards for rounds 0 and 8.
        assert_eq!(expected0 + expected8, compute_rewards(&state, &pay_pre16));

        // Round 16: Alice and Bob finalized the block. Carol only had quorum 50%.
        let assigned = total_weight;
        let reduced_reward = state.params().reduced_block_reward();
        let expected16 = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            reduced_reward * ALICE_CAROL_W * CAROL_W / (assigned * total_weight),
        ]);
        assert_eq!(expected16, compute_rewards(&state, &pay16));

        // Round 16 with faulty Carol: Alice and Bob finalized the block, Carol is unassigned.
        let assigned = ALICE_BOB_W;
        let expected16f = ValidatorMap::from(vec![
            TEST_BLOCK_REWARD * ALICE_BOB_W * ALICE_W / (assigned * total_weight),
            TEST_BLOCK_REWARD * ALICE_BOB_W * BOB_W / (assigned * total_weight),
            0,
        ]);
        assert_eq!(expected16f, compute_rewards(&state, &pay16f));
        Ok(())
    }
}
