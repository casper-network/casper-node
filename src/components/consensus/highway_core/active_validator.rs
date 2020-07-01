use tracing::warn;

use super::{
    state::State,
    validators::ValidatorIndex,
    vertex::{Vertex, WireVote},
    vote::{Observation, Panorama},
};
use crate::components::consensus::consensus_protocol::BlockContext;
use crate::components::consensus::highway_core::vertex::SignedWireVote;
use crate::components::consensus::traits::Context;

/// An action taken by a validator.
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum Effect<C: Context> {
    /// Newly vertex that should be gossiped to peers and added to the protocol state.
    NewVertex(Vertex<C>),
    /// `handle_timer` needs to be called at the specified instant.
    ScheduleTimer(u64),
    /// `propose` needs to be called with a value for a new block with the specified instant.
    // TODO: Add more information required by the deploy buffer.
    RequestNewBlock(BlockContext),
}

/// A validator that actively participates in consensus by creating new vertices.
///
/// It implements the Highway schedule. The protocol proceeds in rounds, and in each round one
/// validator is the _leader_.
/// * In the beginning of the round, the leader sends a _proposal_ vote, containing a consensus
///   value (i.e. a block).
/// * Upon receiving the proposal, all the other validators send a _confirmation_ vote, citing only
///   the proposal, their own previous message, and resulting transitive justifications.
/// * At a fixed point in time later in the round, everyone unconditionally sends a _witness_ vote,
///   citing every vote they have received so far.
///
/// If the rounds are long enough (i.e. message delivery is fast enough) and there are enough
/// honest validators, there will be a lot of confirmations for the proposal, and enough witness
/// votes citing all those confirmations, to create a summit and finalize the proposal.
#[derive(Debug)]
pub(crate) struct ActiveValidator<C: Context> {
    /// Our own validator index.
    vidx: ValidatorIndex,
    /// The validator's secret signing key.
    secret: C::ValidatorSecret,
    /// The round exponent: Our subjective rounds are `1 << round_exp` milliseconds long.
    round_exp: u8,
    /// The latest timer we scheduled.
    next_timer: u64,
}

impl<C: Context> ActiveValidator<C> {
    /// Creates a new `ActiveValidator` and the timer effect for the first call.
    pub(crate) fn new(
        vidx: ValidatorIndex,
        secret: C::ValidatorSecret,
        round_exp: u8,
        instant: u64,
        state: &State<C>,
    ) -> (Self, Vec<Effect<C>>) {
        let mut av = ActiveValidator {
            vidx,
            secret,
            round_exp,
            next_timer: 0,
        };
        let effects = av.schedule_timer(instant, state);
        (av, effects)
    }

    /// Returns actions a validator needs to take at the specified `instant`, with the given
    /// protocol `state`.
    pub(crate) fn handle_timer(&mut self, instant: u64, state: &State<C>) -> Vec<Effect<C>> {
        let mut effects = self.schedule_timer(instant, state);
        if self.earliest_vote_time(state) > instant {
            warn!(%instant, "skipping outdated timer event");
            return effects;
        }
        let round_offset = instant % self.round_len();
        let round_id = instant - round_offset;
        if round_offset == 0 && state.leader(round_id) == self.vidx {
            let bctx = BlockContext { instant };
            effects.push(Effect::RequestNewBlock(bctx));
        } else if round_offset == self.witness_offset() {
            let panorama = state.panorama_cutoff(state.panorama(), instant);
            let witness_vote = self.new_vote(panorama, instant, None, state);
            effects.push(Effect::NewVertex(Vertex::Vote(witness_vote)))
        }
        effects
    }

    /// Returns actions a validator needs to take upon receiving a new vote.
    pub(crate) fn on_new_vote(
        &self,
        vhash: &C::Hash,
        instant: u64,
        state: &State<C>,
    ) -> Vec<Effect<C>> {
        if self.earliest_vote_time(state) > instant {
            warn!(%instant, "skipping outdated confirmation");
        } else if self.should_send_confirmation(vhash, instant, state) {
            let panorama = self.confirmation_panorama(vhash, state);
            let confirmation_vote = self.new_vote(panorama, instant, None, state);
            return vec![Effect::NewVertex(Vertex::Vote(confirmation_vote))];
        }
        vec![]
    }

    /// Proposes a new block with the given consensus value.
    pub(crate) fn propose(
        &self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        state: &State<C>,
    ) -> Vec<Effect<C>> {
        if self.earliest_vote_time(state) > block_context.instant {
            warn!(?block_context, "skipping outdated proposal");
            return vec![];
        }
        let panorama = state.panorama_cutoff(state.panorama(), block_context.instant);
        let instant = block_context.instant();
        let proposal_vote = self.new_vote(panorama, instant, Some(value), state);
        vec![Effect::NewVertex(Vertex::Vote(proposal_vote))]
    }

    /// Returns whether the incoming message is a proposal that we need to send a confirmation for.
    fn should_send_confirmation(&self, vhash: &C::Hash, instant: u64, state: &State<C>) -> bool {
        let vote = state.vote(vhash);
        if vote.instant > instant {
            warn!(%vote.instant, %instant, "added a vote with a future timestamp");
            return false;
        }
        instant / self.round_len() == vote.instant / self.round_len() // Current round.
            && state.leader(vote.instant) == vote.sender // The sender is the round's leader.
            && vote.sender != self.vidx // We didn't send it ourselves.
            && !state.has_evidence(vote.sender) // The sender is not faulty.
            && state
                .panorama()
                .get(self.vidx)
                .correct()
                .map_or(true, |own_vh| {
                    !state.sees_correct(&state.vote(own_vh).panorama, vhash)
                }) // We haven't confirmed it already.
    }

    /// Returns the panorama of the confirmation for the leader vote `vhash`.
    fn confirmation_panorama(&self, vhash: &C::Hash, state: &State<C>) -> Panorama<C> {
        let vote = state.vote(vhash);
        let mut panorama;
        if let Some(prev_hash) = state.panorama().get(self.vidx).correct().cloned() {
            let own_vote = state.vote(&prev_hash);
            panorama = state.merge_panoramas(&vote.panorama, &own_vote.panorama);
            panorama.update(self.vidx, Observation::Correct(prev_hash));
        } else {
            panorama = vote.panorama.clone();
        }
        panorama.update(vote.sender, Observation::Correct(vhash.clone()));
        for faulty_v in state.faulty_validators() {
            panorama.update(faulty_v, Observation::Faulty);
        }
        panorama
    }

    /// Returns a new vote with the given data, and the correct sequence number.
    fn new_vote(
        &self,
        panorama: Panorama<C>,
        instant: u64,
        value: Option<C::ConsensusValue>,
        state: &State<C>,
    ) -> SignedWireVote<C> {
        let add1 = |vh: &C::Hash| state.vote(vh).seq_number + 1;
        let seq_number = panorama.get(self.vidx).correct().map_or(0, add1);
        let wvote = WireVote {
            panorama,
            sender: self.vidx,
            value,
            seq_number,
            instant,
        };
        SignedWireVote::new(wvote, &self.secret)
    }

    /// Returns a `ScheduleTimer` effect for the next time we need to be called.
    fn schedule_timer(&mut self, instant: u64, state: &State<C>) -> Vec<Effect<C>> {
        if self.next_timer > instant {
            return Vec::new(); // We already scheduled the next call; nothing to do.
        }
        let round_offset = instant % self.round_len();
        let round_id = instant - round_offset;
        self.next_timer = if round_offset < self.witness_offset() {
            round_id + self.witness_offset()
        } else if state.leader(round_id + self.round_len()) == self.vidx {
            round_id + self.round_len()
        } else {
            round_id + self.round_len() + self.witness_offset()
        };
        vec![Effect::ScheduleTimer(self.next_timer)]
    }

    /// Returns the earliest instant where we can cast our next vote without equivocating, i.e. the
    /// timestamp of our previous vote, or 0 if there is none.
    fn earliest_vote_time(&self, state: &State<C>) -> u64 {
        let opt_own_vh = state.panorama().get(self.vidx).correct();
        opt_own_vh.map_or(0, |own_vh| state.vote(own_vh).instant)
    }

    /// Returns the number of ticks after the beginning of a round when the witness votes are sent.
    fn witness_offset(&self) -> u64 {
        self.round_len() * 2 / 3
    }

    /// The length of a round, in ticks.
    fn round_len(&self) -> u64 {
        1u64 << self.round_exp
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use super::{
        super::{
            finality_detector::{FinalityDetector, FinalityResult},
            state::{tests::*, AddVoteError, Weight},
            vertex::Vertex,
        },
        *,
    };

    type Eff = Effect<TestContext>;

    impl Eff {
        fn unwrap_vote(self) -> SignedWireVote<TestContext> {
            if let Eff::NewVertex(Vertex::Vote(swvote)) = self {
                swvote
            } else {
                panic!("Unexpected effect: {:?}", self);
            }
        }
    }

    fn unwrap_single<T: Debug>(vec: Vec<T>) -> T {
        let mut iter = vec.into_iter();
        match (iter.next(), iter.next()) {
            (None, _) => panic!("Unexpected empty vec"),
            (Some(t), None) => t,
            (Some(t0), Some(t1)) => panic!("Expected only one element: {:?}, {:?}", t0, t1),
        }
    }

    #[test]
    #[allow(clippy::unreadable_literal)] // 0xC0FFEE is more readable than 0x00C0_FFEE.
    fn active_validator() -> Result<(), AddVoteError<TestContext>> {
        let mut state = State::<TestContext>::new(&[Weight(3), Weight(4)], 0);
        let mut fd = FinalityDetector::new(Weight(2));

        // We start at time 410, with round length 16, so the first leader tick is 416, and the
        // first witness tick 426.
        assert_eq!(ALICE, state.leader(416)); // Alice will be the first leader.
        assert_eq!(BOB, state.leader(432)); // Bob will be the second leader.
        let (mut alice_av, effects) = ActiveValidator::new(ALICE, TestSecret(0), 4, 410, &state);
        assert_eq!([Eff::ScheduleTimer(416)], *effects);
        let (mut bob_av, effects) = ActiveValidator::new(BOB, TestSecret(1), 4, 410, &state);
        assert_eq!([Eff::ScheduleTimer(426)], *effects);

        assert!(alice_av.handle_timer(415, &state).is_empty()); // Too early: No new effects.

        // Alice wants to propose a block, and also make her witness vote at 426.
        let bctx = match &*alice_av.handle_timer(416, &state) {
            [Eff::ScheduleTimer(426), Eff::RequestNewBlock(bctx)] => bctx.clone(),
            effects => panic!("unexpected effects {:?}", effects),
        };
        assert_eq!(416, bctx.instant());

        // She has a pending deploy from Colin who wants to pay for a hot beverage.
        let effects = alice_av.propose(0xC0FFEE, bctx, &state);
        let proposal_wvote = unwrap_single(effects).unwrap_vote();
        let prop_hash = proposal_wvote.hash();
        state.add_vote(proposal_wvote)?;
        assert!(alice_av.on_new_vote(&prop_hash, 417, &state).is_empty());

        // Bob creates a confirmation vote for Alice's proposal.
        let effects = bob_av.on_new_vote(&prop_hash, 419, &state);
        state.add_vote(unwrap_single(effects).unwrap_vote())?;

        // Bob creates his witness message 2/3 through the round.
        let mut effects = bob_av.handle_timer(426, &state).into_iter();
        assert_eq!(Some(Eff::ScheduleTimer(432)), effects.next()); // Bob is the next leader.
        state.add_vote(effects.next().unwrap().unwrap_vote())?;
        assert_eq!(None, effects.next());

        assert_eq!(FinalityResult::None, fd.run(&state)); // Alice has not witnessed Bob's vote yet.

        // Alice also sends her own witness message, completing the summit for her proposal.
        let mut effects = alice_av.handle_timer(426, &state).into_iter();
        assert_eq!(Some(Eff::ScheduleTimer(442)), effects.next()); // Timer for witness vote.
        state.add_vote(effects.next().unwrap().unwrap_vote())?;
        assert_eq!(None, effects.next());

        // Payment finalized! "One Pumpkin Spice Mochaccino for Corbyn!"
        assert_eq!(
            FinalityResult::Finalized(0xC0FFEE, Vec::new()),
            fd.run(&state)
        );
        Ok(())
    }
}
