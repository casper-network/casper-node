//! # A simple consensus protocol.
//!
//! This protocol requires that at most _f_ out of _n > 3 f_ validators (by weight) are faulty. It
//! also assumes that there is an upper bound for the network delay: how long a message sent by a
//! correct validator can take before it is delivered.
//!
//! Under these conditions all correct nodes will reach agreement on a chain of _finalized_ blocks.
//!
//! A _quorum_ is a set of validators whose total weight is greater than _(n + f) / 2_. Thus any two
//! quorums always have a correct validator in common. Since _(n + f) / 2 < n - f_, the correct
//! validators constitute a quorum.
//!
//!
//! ## How it Works
//!
//! In every round the designated leader can sign a `Proposal` message to suggest a block. The
//! proposal also points to an earlier round in which the parent block was proposed.
//!
//! Each validator then signs an `Echo` message with the proposal's hash. Correct validators only
//! sign one `Echo` per round, so at most one proposal can get `Echo`s signed by a quorum. If there
//! is a quorum and some other conditions are met (see below), the proposal is _accepted_. The next
//! round's leader can now make a proposal that uses this one as a parent.
//!
//! Each validator that observes the proposal to be accepted in time signs a `Vote(true)` message.
//! If they time out waiting they sign `Vote(false)` instead. If a quorum signs `true`, the round is
//! _committed_ and the proposal and all its ancestors are finalized. If a quorum signs `false`, the
//! round is _skippable_: The next round's leader can now make a proposal with a parent from an
//! earlier round. Correct validators only sign either `true` or `false`, so a round can be either
//! committed or skippable but not both.
//!
//! If there is no accepted proposal all correct validators will eventually vote `false`, so the
//! round becomes skippable. This is what makes the protocol _live_: The next leader will eventually
//! be allowed to make a proposal, because either there is an accepted proposal that can be the
//! parent, or the round will eventually be skippable and an earlier round's proposal can be used as
//! a parent. If the timeout is long enough correct proposers' blocks will usually get finalized.
//!
//! For a proposal to be _accepted_, the parent proposal needs to also be accepted, and all rounds
//! between the parent and the current round must be skippable. This is what makes the protocol
//! _safe_: If two rounds are committed, their proposals must be ancestors of each other,
//! because they are not skippable. Thus no two conflicting blocks can become finalized.
//!
//! Of course there is also a first block: Whenever _all_ earlier rounds are skippable (in
//! particular in the first round) the leader may propose a block with no parent.
//!
//!
//! ## Syncing the State
//!
//! Every new signed message is optimistically sent directly to all peers. We want to guarantee that
//! it is eventually seen by all validators, even if they are not fully connected. This is
//! achieved via a pull-based randomized gossip mechanism:
//!
//! A `SyncState` message containing information about a random part of the local protocol state is
//! periodically sent to a random peer. The peer compares that to its local state, and responds with
//! all signed messages that it has and the other is missing.

#[cfg(test)]
mod tests;

use std::{
    any::Any,
    cmp::Reverse,
    collections::{btree_map, BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Debug,
    iter,
    path::PathBuf,
};

use datasize::DataSize;
use itertools::Itertools;
use rand::{seq::IteratorRandom, Rng};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use casper_types::{system::auction::BLOCK_REWARD, U512};

use crate::{
    components::consensus::{
        config::Config,
        consensus_protocol::{
            BlockContext, ConsensusProtocol, FinalizedBlock, ProposedBlock, ProtocolOutcome,
            ProtocolOutcomes, TerminalBlockData,
        },
        highway_core::{
            state::{weight::Weight, Params},
            validators::{ValidatorIndex, ValidatorMap, Validators},
        },
        protocols,
        traits::{ConsensusValueT, Context, ValidatorSecret},
        ActionId, LeaderSequence, TimerId,
    },
    types::{Chainspec, NodeId, TimeDiff, Timestamp},
    utils::{div_round, ds},
    NodeRng,
};

/// The timer for syncing with a random peer.
const TIMER_ID_SYNC_PEER: TimerId = TimerId(0);
/// The timer for calling `update`.
const TIMER_ID_UPDATE: TimerId = TimerId(1);
/// The timer for logging inactive validators.
const TIMER_ID_LOG_PARTICIPATION: TimerId = TimerId(2);
/// The timer for an alert no progress was made in a long time.
const TIMER_ID_STANDSTILL_ALERT: TimerId = TimerId(3);

/// The maximum number of future rounds we instantiate if we get messages from rounds that we
/// haven't started yet.
const MAX_FUTURE_ROUNDS: u32 = 10;

/// Identifies a single [`Round`] in the protocol.
pub(crate) type RoundId = u32;

/// The protocol proceeds in rounds, for each of which we must
/// keep track of proposals, echos, votes, and the current outcome
/// of the round.
#[derive(Debug, DataSize)]
pub(crate) struct Round<C>
where
    C: Context,
{
    /// All of the proposals sent to us this round from the leader
    #[data_size(with = ds::hashmap_sample)]
    proposals: HashMap<C::Hash, (Proposal<C>, C::Signature)>,
    /// The echos we've received for each proposal so far.
    #[data_size(with = ds::hashmap_sample)]
    echos: HashMap<C::Hash, BTreeMap<ValidatorIndex, C::Signature>>,
    /// The votes we've received for this round so far.
    votes: BTreeMap<bool, ValidatorMap<Option<C::Signature>>>,
    /// The memoized results in this round.
    outcome: RoundOutcome<C>,
}

impl<C: Context> Round<C> {
    /// Creates a new [`Round`] with no proposals, echos, votes, and empty
    /// round outcome.
    fn new(validator_count: usize) -> Round<C> {
        let mut votes = BTreeMap::new();
        votes.insert(false, vec![None; validator_count].into());
        votes.insert(true, vec![None; validator_count].into());
        Round {
            proposals: HashMap::new(),
            echos: HashMap::new(),
            votes,
            outcome: RoundOutcome::default(),
        }
    }

    /// Inserts a `Proposal` and returns `false` if we already had it.
    fn insert_proposal(&mut self, proposal: Proposal<C>, signature: C::Signature) -> bool {
        let hash = proposal.hash();
        self.proposals.insert(hash, (proposal, signature)).is_none()
    }

    /// Inserts an `Echo`; returns `false` if we already had it.
    fn insert_echo(
        &mut self,
        hash: C::Hash,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) -> bool {
        self.echos
            .entry(hash)
            .or_insert_with(BTreeMap::new)
            .insert(validator_idx, signature)
            .is_none()
    }

    /// Inserts a `Vote`; returns `false` if we already had it.
    fn insert_vote(
        &mut self,
        vote: bool,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) -> bool {
        // Safe to unwrap: Both `true` and `false` entries were created in `new`.
        let votes_map = self.votes.get_mut(&vote).unwrap();
        if votes_map[validator_idx].is_none() {
            votes_map[validator_idx] = Some(signature);
            true
        } else {
            false
        }
    }

    /// Returns the accepted proposal, if any, together with its height.
    fn accepted_proposal(&self) -> Option<(u64, &Proposal<C>)> {
        let height = self.outcome.accepted_proposal_height?;
        let hash = self.outcome.quorum_echos?;
        let (proposal, _signature) = self.proposals.get(&hash)?;
        Some((height, proposal))
    }
}

impl<C: Context> Round<C> {
    /// Check if the round has already received this message.
    fn contains(&self, content: &Content<C>, validator_idx: ValidatorIndex) -> bool {
        match content {
            Content::Proposal(proposal) => self.proposals.contains_key(&proposal.hash()),
            Content::Echo(hash) => self
                .echos
                .get(hash)
                .map_or(false, |echo_map| echo_map.contains_key(&validator_idx)),
            Content::Vote(vote) => self.votes[vote][validator_idx].is_some(),
        }
    }
}

/// Contains the state required for the protocol.
#[derive(DataSize, Debug)]
#[allow(clippy::type_complexity)] // TODO
pub(crate) struct SimpleConsensus<C>
where
    C: Context,
{
    /// Contains numerical parameters for the protocol
    /// TODO currently using Highway params
    params: Params,
    /// Identifies this instance of the protocol uniquely
    instance_id: C::InstanceId,
    /// The timeout for the current round's proposal
    proposal_timeout: TimeDiff,
    /// The validators in this instantiation of the protocol
    validators: Validators<C::ValidatorId>,
    /// If we are a validator ourselves, we must know which index we
    /// are in the [`Validators`] and have a private key for consensus.
    active_validator: Option<(ValidatorIndex, C::ValidatorSecret)>,
    /// When an era has already completed, sometimes we still need to keep
    /// it around to provide evidence for equivocation in previous eras.
    evidence_only: bool,
    /// Proposals which have not yet had their parent accepted yet.
    proposals_waiting_for_parent:
        HashMap<RoundId, HashMap<Proposal<C>, HashSet<(RoundId, NodeId, C::Signature)>>>,
    /// Incoming blocks we can't add yet because we are waiting for validation.
    proposals_waiting_for_validation:
        HashMap<ProposedBlock<C>, HashSet<(RoundId, Proposal<C>, NodeId, C::Signature)>>,
    /// If we requested a new block from the block proposer component this contains the proposal's
    /// round ID and the parent's round ID, if there is a parent.
    pending_proposal: Option<(BlockContext<C>, RoundId, Option<RoundId>)>,
    leader_sequence: LeaderSequence,
    /// The [`Round`]s of this protocol which we've instantiated.
    rounds: BTreeMap<RoundId, Round<C>>,
    /// List of faulty validators and their type of fault.
    faults: HashMap<ValidatorIndex, Fault<C>>,
    /// The threshold weight above which we are not fault tolerant any longer.
    ftt: Weight,
    /// The configuration for the protocol
    /// TODO currently using Highway config
    config: super::highway::config::Config,
    /// The validator's voting weights.
    weights: ValidatorMap<Weight>,
    /// This is `true` for every validator we have received a signature from or we know to be
    /// faulty.
    active: ValidatorMap<bool>,
    /// The lowest round ID of a block that could still be finalized in the future.
    first_non_finalized_round_id: RoundId,
    /// The lowest round that needs to be considered in `upgrade`.
    maybe_dirty_round_id: Option<RoundId>,
    /// The lowest non-skippable round without an accepted value.
    current_round: RoundId,
    /// The timeout for the current round.
    current_timeout: Timestamp,
    /// Whether anything was recently added to the protocol state.
    progress_detected: bool,
    /// Whether or not the protocol is currently paused
    paused: bool,
}

impl<C: Context + 'static> SimpleConsensus<C> {
    /// Creates a new [`SimpleConsensus`] instance.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn new(
        instance_id: C::InstanceId,
        validator_stakes: BTreeMap<C::ValidatorId, U512>,
        faulty: &HashSet<C::ValidatorId>,
        inactive: &HashSet<C::ValidatorId>,
        chainspec: &Chainspec,
        config: &Config,
        prev_cp: Option<&dyn ConsensusProtocol<C>>,
        era_start_time: Timestamp,
        seed: u64,
        _now: Timestamp,
    ) -> SimpleConsensus<C> {
        let validators = protocols::common::validators::<C>(faulty, inactive, validator_stakes);
        let weights = protocols::common::validator_weights::<C>(&validators);
        let mut active: ValidatorMap<bool> = weights.iter().map(|_| false).collect();
        let highway_config = &chainspec.highway_config;
        let ftt =
            protocols::common::ftt::<C>(highway_config.finality_threshold_fraction, &validators);

        // Use the estimate from the previous era as the proposal timeout. Start with one minimum
        // round length.
        let proposal_timeout = prev_cp
            .and_then(|cp| cp.as_any().downcast_ref::<SimpleConsensus<C>>())
            .map(|sc| sc.proposal_timeout)
            .unwrap_or_else(|| highway_config.min_round_length());

        let mut can_propose: ValidatorMap<bool> = weights.iter().map(|_| true).collect();
        for vidx in validators.iter_cannot_propose_idx() {
            can_propose[vidx] = false;
        }
        let faults: HashMap<_, _> = validators
            .iter_banned_idx()
            .map(|idx| (idx, Fault::Banned))
            .collect();
        let leader_sequence = LeaderSequence::new(seed, &weights, can_propose);

        for idx in faults.keys() {
            active[*idx] = true;
        }

        info!(
            %proposal_timeout,
            "initializing SimpleConsensus instance",
        );

        let params = Params::new(
            seed,
            BLOCK_REWARD,
            (highway_config.reduced_reward_multiplier * BLOCK_REWARD).to_integer(),
            highway_config.minimum_round_exponent,
            highway_config.maximum_round_exponent,
            highway_config.minimum_round_exponent,
            chainspec.core_config.minimum_era_height,
            era_start_time,
            era_start_time + chainspec.core_config.era_duration,
            0,
        );

        SimpleConsensus {
            leader_sequence,
            proposals_waiting_for_parent: HashMap::new(),
            proposals_waiting_for_validation: HashMap::new(),
            rounds: BTreeMap::new(),
            first_non_finalized_round_id: 0,
            maybe_dirty_round_id: None,
            current_round: 0,
            current_timeout: Timestamp::from(u64::MAX),
            evidence_only: false,
            faults,
            active,
            config: config.highway.clone(),
            params,
            instance_id,
            proposal_timeout,
            validators,
            ftt,
            active_validator: None,
            weights,
            pending_proposal: None,
            progress_detected: false,
            paused: false,
        }
    }

    /// Creates a new boxed [`SimpleConsensus`] instance.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub(crate) fn new_boxed(
        instance_id: C::InstanceId,
        validator_stakes: BTreeMap<C::ValidatorId, U512>,
        faulty: &HashSet<C::ValidatorId>,
        inactive: &HashSet<C::ValidatorId>,
        chainspec: &Chainspec,
        config: &Config,
        prev_cp: Option<&dyn ConsensusProtocol<C>>,
        era_start_time: Timestamp,
        seed: u64,
        now: Timestamp,
    ) -> (Box<dyn ConsensusProtocol<C>>, ProtocolOutcomes<C>) {
        let sc = Self::new(
            instance_id,
            validator_stakes,
            faulty,
            inactive,
            chainspec,
            config,
            prev_cp,
            era_start_time,
            seed,
            now,
        );
        (Box::new(sc), vec![])
    }

    /// Prints a log statement listing the inactive and faulty validators.
    #[allow(clippy::integer_arithmetic)] // We use u128 to prevent overflows in weight
    fn log_participation(&self) {
        let mut inactive_w = 0;
        let mut faulty_w = 0;
        let total_w = u128::from(self.validators.total_weight().0);
        let mut inactive_validators = Vec::new();
        let mut faulty_validators = Vec::new();
        for (idx, v_id) in self.validators.enumerate_ids() {
            if let Some(status) = ParticipationStatus::for_index(idx, self) {
                match status {
                    ParticipationStatus::Equivocated
                    | ParticipationStatus::EquivocatedInOtherEra => {
                        faulty_w += u128::from(self.weights[idx].0);
                        faulty_validators.push((idx, v_id.clone(), status));
                    }
                    ParticipationStatus::Inactive | ParticipationStatus::LastSeenInRound(_) => {
                        inactive_w += u128::from(self.weights[idx].0);
                        inactive_validators.push((idx, v_id.clone(), status));
                    }
                }
            }
        }
        inactive_validators.sort_by_key(|(idx, _, status)| (Reverse(*status), *idx));
        faulty_validators.sort_by_key(|(idx, _, status)| (Reverse(*status), *idx));
        let participation = Participation::<C> {
            instance_id: self.instance_id,
            inactive_stake_percent: div_round(inactive_w * 100, total_w) as u8,
            faulty_stake_percent: div_round(faulty_w * 100, total_w) as u8,
            inactive_validators,
            faulty_validators,
        };
        info!(?participation, "validator participation");
    }

    /// Returns whether the switch block has already been finalized.
    fn finalized_switch_block(&self) -> bool {
        if let Some(round_id) = self.first_non_finalized_round_id.checked_sub(1) {
            self.accepted_switch_block(round_id)
        } else {
            false
        }
    }

    /// Returns whether a block was accepted that, if finalized, would be the last one.
    fn accepted_switch_block(&self, round_id: RoundId) -> bool {
        match self.round(round_id).and_then(Round::accepted_proposal) {
            None => false,
            Some((height, proposal)) => {
                height.saturating_add(1) >= self.params.end_height()
                    && proposal.timestamp >= self.params.end_timestamp()
            }
        }
    }

    /// Returns whether a proposal without a block was accepted, i.e. whether some ancestor of the
    /// accepted proposal is a switch block.
    fn accepted_dummy_proposal(&self, round_id: RoundId) -> bool {
        match self.round(round_id).and_then(Round::accepted_proposal) {
            None => false,
            Some((_, proposal)) => proposal.maybe_block.is_none(),
        }
    }

    /// Returns whether the validator has already sent an `Echo` in this round.
    fn has_echoed(&self, round_id: RoundId, validator_idx: ValidatorIndex) -> bool {
        self.round(round_id).map_or(false, |round| {
            round
                .echos
                .values()
                .any(|echo_map| echo_map.contains_key(&validator_idx))
        })
    }

    /// Returns whether the validator has already cast a `true` or `false` vote.
    fn has_voted(&self, round_id: RoundId, validator_idx: ValidatorIndex) -> bool {
        self.round(round_id).map_or(false, |round| {
            round.votes[&true][validator_idx].is_some()
                || round.votes[&false][validator_idx].is_some()
        })
    }

    /// Request the latest state from a random peer.
    fn handle_sync_peer_timer(&self, now: Timestamp, rng: &mut NodeRng) -> ProtocolOutcomes<C> {
        if self.evidence_only || self.finalized_switch_block() {
            return vec![]; // Era has ended. No further progress is expected.
        }
        debug!(
            instance_id = ?self.instance_id,
            "syncing with random peer",
        );
        // Inform a peer about our protocol state and schedule the next request.
        let first_validator_idx = ValidatorIndex(rng.gen_range(0..self.weights.len() as u32));
        let round_id = (self.first_non_finalized_round_id..=self.current_round)
            .choose(rng)
            .unwrap_or(self.current_round);
        let payload = self
            .create_sync_state_message(first_validator_idx, round_id)
            .serialize();
        let mut outcomes = vec![ProtocolOutcome::CreatedMessageToRandomPeer(payload)];
        // Periodically sync the state with a random peer.
        // TODO: In this protocol the interval should be shorter than in Highway.
        if let Some(interval) = self.config.request_state_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()) + interval / 100,
                TIMER_ID_SYNC_PEER,
            ));
        }
        outcomes
    }

    /// Returns a `StandstillAlert` if no progress was made; otherwise schedules the next check.
    fn handle_standstill_alert_timer(&mut self, now: Timestamp) -> ProtocolOutcomes<C> {
        if self.evidence_only || self.finalized_switch_block() {
            // Era has ended and no further progress is expected, or shutdown on standstill is
            // turned off.
            return vec![];
        }
        let timeout = match self.config.standstill_timeout {
            None => return vec![],
            Some(timeout) => timeout,
        };
        if !self.progress_detected {
            return vec![ProtocolOutcome::StandstillAlert]; // No progress within the timeout.
        }
        self.progress_detected = false;
        debug!(
            instance_id = ?self.instance_id,
            "progress detected; scheduling next standstill check in {}",
            timeout,
        );
        // Schedule the next standstill check.
        vec![ProtocolOutcome::ScheduleTimer(
            now + timeout,
            TIMER_ID_STANDSTILL_ALERT,
        )]
    }

    /// Prints a log message if the message is a proposal.
    fn log_proposal(&self, proposal: &Proposal<C>, creator_index: ValidatorIndex, msg: &str) {
        let creator = if let Some(creator) = self.validators.id(creator_index) {
            creator
        } else {
            error!(?proposal, ?creator_index, "{}: invalid creator", msg);
            return;
        };
        info!(
            hash = ?proposal.hash(),
            ?creator,
            creator_index = creator_index.0,
            timestamp = %proposal.timestamp,
            "{}", msg,
        );
    }

    fn create_sync_state_message(
        &self,
        first_validator_idx: ValidatorIndex,
        round_id: RoundId,
    ) -> Message<C> {
        let faulty = self.validator_bit_field(first_validator_idx, self.faults.keys().cloned());
        let round = match self.round(round_id) {
            Some(round) => round,
            None => {
                return Message::new_empty_round_sync_state(
                    round_id,
                    first_validator_idx,
                    faulty,
                    self.instance_id,
                );
            }
        };
        let true_votes =
            self.validator_bit_field(first_validator_idx, round.votes[&true].keys_some());
        let false_votes =
            self.validator_bit_field(first_validator_idx, round.votes[&false].keys_some());
        let proposal_hash = round.outcome.quorum_echos.or_else(|| {
            round
                .echos
                .iter()
                .max_by_key(|(_, echo_map)| self.sum_weights(echo_map.keys()))
                .map(|(hash, _)| *hash)
        });
        let mut echos = 0;
        let mut proposal = false;
        if let Some(hash) = proposal_hash {
            echos =
                self.validator_bit_field(first_validator_idx, round.echos[&hash].keys().cloned());
            proposal = round.proposals.contains_key(&hash);
        }
        Message::SyncState {
            round_id,
            proposal_hash,
            proposal,
            first_validator_idx,
            echos,
            true_votes,
            false_votes,
            faulty,
            instance_id: self.instance_id,
        }
    }

    /// Returns a bit field where each bit stands for a validator: the least significant one for
    /// `first_idx` and the most significant one for `fist_idx + 127`, wrapping around at the total
    /// number of validators. The bits of the validators in `index_iter` that fall into that
    /// range are set to `1`, the others are `0`.
    #[allow(clippy::integer_arithmetic)] // TODO
    fn validator_bit_field(
        &self,
        ValidatorIndex(first_idx): ValidatorIndex,
        index_iter: impl Iterator<Item = ValidatorIndex>,
    ) -> u128 {
        let mut bit_field = 0;
        let validator_count = self.weights.len() as u32;
        for ValidatorIndex(v_idx) in index_iter {
            // The validator's bit is v_idx - first_idx, but we wrap around.
            let i = v_idx
                .checked_sub(first_idx)
                .unwrap_or(v_idx + validator_count - first_idx);
            if i < 128 {
                bit_field |= 1 << i; // Set bit number i to 1.
            }
        }
        bit_field
    }

    /// Returns an iterator over all validator indexes whose bits in the `bit_field` are `1`, where
    /// the least significant one stands for `first_idx` and the most significant one for
    /// `first_idx + 127`, wrapping around.
    #[allow(clippy::integer_arithmetic)] // TODO
    fn iter_validator_bit_field(
        &self,
        first_idx: ValidatorIndex,
        mut bit_field: u128,
    ) -> impl Iterator<Item = ValidatorIndex> {
        let validator_count = self.weights.len() as u32;
        let mut idx = first_idx.0; // The last bit stands for first_idx.
        iter::from_fn(move || {
            if bit_field == 0 {
                return None; // No remaining bits with value 1.
            }
            let zeros = bit_field.trailing_zeros();
            // The index of the validator whose bit is 1. We shift the bits to the right so that the
            // least significant bit now corresponds to this one, then we output the index and set
            // the bit to 0.
            idx = (idx + zeros) % validator_count;
            bit_field >>= zeros;
            bit_field &= !1;
            Some(ValidatorIndex(idx))
        })
    }

    /// Returns the leader in the specified round.
    pub(crate) fn leader(&self, round_id: RoundId) -> ValidatorIndex {
        self.leader_sequence.leader(u64::from(round_id))
    }

    /// If we are an active validator and it would be safe for us to sign this message and we
    /// haven't signed it before, we sign it, add it to our state and gossip it to the network.
    ///
    /// Does not call `update`!
    fn create_message(&mut self, round_id: RoundId, content: Content<C>) -> ProtocolOutcomes<C> {
        let (validator_idx, secret_key) =
            if let Some((validator_idx, secret_key)) = &self.active_validator {
                (*validator_idx, secret_key)
            } else {
                return vec![];
            };
        if self.paused {
            return vec![];
        }
        let already_signed = match &content {
            Content::Proposal(_) => {
                if self.leader(round_id) != validator_idx {
                    error!(
                        %round_id, validator_idx = %validator_idx.0,
                        "Tried to create a proposal in wrong round."
                    );
                    return vec![]; // Not creating a proposal: We're not the leader.
                }
                self.round(round_id)
                    .map_or(false, |round| !round.proposals.is_empty())
            }
            Content::Echo(_) => self.has_echoed(round_id, validator_idx),
            Content::Vote(_) => self.has_voted(round_id, validator_idx),
        };
        if already_signed {
            return vec![]; // Not creating message, so we don't double-sign or duplicate.
        }
        let serialized_fields =
            bincode::serialize(&(round_id, &self.instance_id, &content, validator_idx))
                .expect("failed to serialize fields");
        let hash = <C as Context>::hash(&serialized_fields);
        let signature = secret_key.sign(&hash);
        self.add_content(round_id, content.clone(), validator_idx, signature);
        let message = Message::Signed {
            round_id,
            instance_id: self.instance_id,
            content,
            validator_idx,
            signature,
        };
        vec![ProtocolOutcome::CreatedGossipMessage(message.serialize())]
    }

    /// When we receive evidence for a fault, we must notify the rest of the network of this
    /// evidence. Beyond that, we can remove all of the faulty validator's previous information
    /// from the protocol state.
    fn handle_fault(
        &mut self,
        round_id: RoundId,
        validator_idx: ValidatorIndex,
        (content0, signature0): (Content<C>, C::Signature),
        (content1, signature1): (Content<C>, C::Signature),
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let validator_id = if let Some(validator_id) = self.validators.id(validator_idx) {
            validator_id.clone()
        } else {
            error!("invalid validator index");
            return vec![];
        };
        info!(index = validator_idx.0, id = %validator_id, "validator double-signed");
        if let Some(Fault::Direct(_, _)) = self.faults.get(&validator_idx) {
            return vec![]; // Validator is already known to be faulty.
        }
        let msg0 = Message::Signed {
            round_id,
            instance_id: self.instance_id,
            content: content0,
            validator_idx,
            signature: signature0,
        };
        let msg1 = Message::Signed {
            round_id,
            instance_id: self.instance_id,
            content: content1,
            validator_idx,
            signature: signature1,
        };
        // TODO should we send this as one message?
        let mut outcomes = vec![
            ProtocolOutcome::CreatedGossipMessage(msg0.serialize()),
            ProtocolOutcome::CreatedGossipMessage(msg1.serialize()),
            ProtocolOutcome::NewEvidence(validator_id),
        ];
        self.faults.insert(validator_idx, Fault::Direct(msg0, msg1));
        self.active[validator_idx] = true;
        self.progress_detected = true;
        if self.faulty_weight() > self.ftt {
            outcomes.push(ProtocolOutcome::FttExceeded);
            return outcomes;
        }

        // Remove all Votes and Echos from the faulty validator: They count towards every quorum now
        // so nobody has to store their messages.
        for round in self.rounds.values_mut() {
            round.votes.get_mut(&false).unwrap()[validator_idx] = None;
            round.votes.get_mut(&true).unwrap()[validator_idx] = None;
            round.echos.retain(|_, echo_map| {
                echo_map.remove(&validator_idx);
                !echo_map.is_empty()
            });
        }

        // Recompute quorums; if any new quorums are found, call `update`.
        for round_id in
            self.first_non_finalized_round_id..=self.rounds.keys().last().copied().unwrap_or(0)
        {
            if !self.rounds.contains_key(&round_id) {
                continue;
            }
            if self.rounds[&round_id].outcome.quorum_echos.is_none() {
                let hashes = self.rounds[&round_id].echos.keys().copied().collect_vec();
                if hashes
                    .into_iter()
                    .any(|hash| self.check_new_echo_quorum(round_id, hash))
                {
                    self.mark_dirty(round_id);
                }
            }
            if self.check_new_vote_quorum(round_id, true)
                || self.check_new_vote_quorum(round_id, false)
            {
                self.mark_dirty(round_id);
            }
        }
        outcomes.extend(self.update(now));
        outcomes
    }

    /// When we receive a request to synchronize, we must take a careful diff of our state and the
    /// state in the sync state to ensure we send them exactly what they need to get back up to
    /// speed in the network.
    #[allow(clippy::too_many_arguments)] // TODO
    fn handle_sync_state(
        &self,
        round_id: RoundId,
        proposal_hash: Option<C::Hash>,
        has_proposal: bool,
        first_validator_idx: ValidatorIndex,
        echos: u128,
        true_votes: u128,
        false_votes: u128,
        faulty: u128,
        sender: NodeId,
    ) -> ProtocolOutcomes<C> {
        // TODO: Limit how much time and bandwidth we spend on each peer.
        // TODO: Send only enough signatures for quorum.
        // TODO: Combine multiple `SignedMessage`s with the same values into one.
        // TODO: Refactor to something more readable!!
        let round = match self.round(round_id) {
            Some(round) => round,
            None => return vec![],
        };
        if first_validator_idx.0 >= self.weights.len() as u32 {
            info!(
                first_validator_idx = first_validator_idx.0,
                ?sender,
                "invalid SyncState message"
            );
            return vec![];
        }
        let our_faulty = self.validator_bit_field(first_validator_idx, self.faults.keys().cloned());
        let mut contents = vec![];
        if let Some(hash) = proposal_hash {
            if let Some(echo_map) = round.echos.get(&hash) {
                let our_echos =
                    self.validator_bit_field(first_validator_idx, echo_map.keys().cloned());
                let missing_echos = our_echos & !(echos | faulty | our_faulty);
                for v_idx in self.iter_validator_bit_field(first_validator_idx, missing_echos) {
                    contents.push((Content::Echo(hash), v_idx, echo_map[&v_idx]));
                }
            }
            if !has_proposal {
                if let Some((proposal, signature)) = round.proposals.get(&hash) {
                    let content = Content::Proposal(proposal.clone());
                    contents.push((content, self.leader(round_id), *signature));
                }
            }
        } else if let Some(hash) = round.outcome.quorum_echos {
            for (v_idx, signature) in &round.echos[&hash] {
                contents.push((Content::Echo(hash), *v_idx, *signature));
            }
        }
        let our_true_votes =
            self.validator_bit_field(first_validator_idx, round.votes[&true].keys_some());
        let missing_true_votes = our_true_votes & !(true_votes | faulty | our_faulty);
        for v_idx in self.iter_validator_bit_field(first_validator_idx, missing_true_votes) {
            let signature = round.votes[&true][v_idx].unwrap();
            contents.push((Content::Vote(true), v_idx, signature));
        }
        let our_false_votes =
            self.validator_bit_field(first_validator_idx, round.votes[&false].keys_some());
        let missing_false_votes = our_false_votes & !(false_votes | faulty | our_faulty);
        for v_idx in self.iter_validator_bit_field(first_validator_idx, missing_false_votes) {
            let signature = round.votes[&false][v_idx].unwrap();
            contents.push((Content::Vote(false), v_idx, signature));
        }
        let mut outcomes = contents
            .into_iter()
            .map(|(content, validator_idx, signature)| {
                let msg = Message::Signed {
                    round_id,
                    instance_id: self.instance_id,
                    content,
                    validator_idx,
                    signature,
                };
                ProtocolOutcome::CreatedTargetedMessage(msg.serialize(), sender)
            })
            .collect_vec();
        let missing_faulty = our_faulty & !faulty;
        for v_idx in self.iter_validator_bit_field(first_validator_idx, missing_faulty) {
            match &self.faults[&v_idx] {
                Fault::Banned => {
                    error!(
                        validator_index = v_idx.0,
                        ?sender,
                        "peer disagrees about banned validator"
                    );
                }
                Fault::Direct(msg0, msg1) => {
                    outcomes.push(ProtocolOutcome::CreatedTargetedMessage(
                        msg0.serialize(),
                        sender,
                    ));
                    outcomes.push(ProtocolOutcome::CreatedTargetedMessage(
                        msg1.serialize(),
                        sender,
                    ));
                }
                Fault::Indirect => {
                    let vid = self.validators.id(v_idx).unwrap().clone();
                    outcomes.push(ProtocolOutcome::SendEvidence(sender, vid));
                }
            }
        }
        outcomes
    }

    /// The main entry point for non-synchronization messages. This function mostly authenticates
    /// and authorizes the message, passing it to [`add_content`] if it passes snuff for the
    /// main protocol logic.
    #[allow(clippy::too_many_arguments)]
    fn handle_signed_message(
        &mut self,
        msg: Vec<u8>,
        round_id: RoundId,
        content: Content<C>,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
        sender: NodeId,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        // TODO: Error handling.
        let err_msg = |message: &'static str| {
            vec![ProtocolOutcome::InvalidIncomingMessage(
                msg.clone(),
                sender,
                anyhow::Error::msg(message),
            )]
        };

        let validator_id = if let Some(validator_id) = self.validators.id(validator_idx) {
            validator_id.clone()
        } else {
            return err_msg("invalid validator index");
        };

        if let Some(fault) = self.faults.get(&validator_idx) {
            if fault.is_banned() || !content.is_proposal() {
                debug!(?validator_id, "ignoring message from faulty validator");
                return vec![];
            }
        }

        if round_id > self.current_round.saturating_add(MAX_FUTURE_ROUNDS) {
            debug!(%round_id, "dropping message from future round");
            return vec![];
        }

        if self.evidence_only {
            debug!("received an irrelevant message");
            // TODO: Return vec![] if this isn't an evidence message.
        }

        if self
            .round(round_id)
            .map_or(false, |round| round.contains(&content, validator_idx))
        {
            debug!(
                ?round_id,
                ?content,
                validator_idx = validator_idx.0,
                "received a duplicated message"
            );
            return vec![];
        }

        let serialized_fields =
            bincode::serialize(&(round_id, &self.instance_id, &content, validator_idx))
                .expect("failed to serialize fields");
        let hash = <C as Context>::hash(&serialized_fields);
        if !C::verify_signature(&hash, &validator_id, &signature) {
            return err_msg("invalid signature");
        }

        let mut outcomes = vec![];

        if let Some((content2, signature2)) = self.detect_fault(round_id, validator_idx, &content) {
            outcomes.extend(self.handle_fault(
                round_id,
                validator_idx,
                (content.clone(), signature),
                (content2, signature2),
                now,
            ));
            if !content.is_proposal() {
                debug!(?validator_id, "ignoring message from faulty validator");
                return outcomes;
            }
        }

        match content {
            Content::Proposal(proposal) => {
                if proposal.timestamp > now + self.config.pending_vertex_timeout {
                    trace!("received a proposal with a timestamp far in the future; dropping");
                    return outcomes;
                }
                if proposal.timestamp > now {
                    trace!("received a proposal with a timestamp slightly in the future");
                    // TODO: If it's not from an equivocator and from the future, add to queue
                    // trace!("received a proposal from the future; storing for later");
                    // let timer_id = TIMER_ID_VERTEX_WITH_FUTURE_TIMESTAMP;
                    // vec![ProtocolOutcome::ScheduleTimer(timestamp, timer_id)]
                }

                if validator_idx != self.leader(round_id) {
                    outcomes.extend(err_msg("wrong leader"));
                    return outcomes;
                }
                if proposal
                    .maybe_parent_round_id
                    .map_or(false, |parent_round_id| parent_round_id >= round_id)
                {
                    outcomes.extend(err_msg(
                        "invalid proposal: parent is not from an earlier round",
                    ));
                    return outcomes;
                }

                if (proposal.maybe_parent_round_id.is_none() || proposal.maybe_block.is_none())
                    != proposal.inactive.is_none()
                {
                    outcomes.extend(err_msg(
                        "invalid proposal: inactive must be present in all except the first and \
                        dummy proposals",
                    ));
                    return outcomes;
                }
                if let Some(inactive) = &proposal.inactive {
                    if inactive
                        .iter()
                        .any(|idx| *idx == validator_idx || !self.weights.has(*idx))
                    {
                        outcomes.extend(err_msg(
                            "invalid proposal: invalid inactive validator index",
                        ));
                        return outcomes;
                    }
                }

                let ancestor_values = if let Some(parent_round_id) = proposal.maybe_parent_round_id
                {
                    if let Some(ancestor_values) = self.ancestor_values(parent_round_id) {
                        ancestor_values
                    } else {
                        self.proposals_waiting_for_parent
                            .entry(parent_round_id)
                            .or_insert_with(HashMap::new)
                            .entry(proposal)
                            .or_insert_with(HashSet::new)
                            .insert((round_id, sender, signature));
                        return outcomes;
                    }
                } else {
                    vec![]
                };

                outcomes.extend(self.validate_proposal(
                    round_id,
                    proposal,
                    ancestor_values,
                    sender,
                    signature,
                ));
            }
            content @ Content::Echo(_) | content @ Content::Vote(_) => {
                self.add_content(round_id, content, validator_idx, signature);
            }
        }
        outcomes.extend(self.update(now));
        outcomes
    }

    /// Updates the round's outcome and returns `true` if there is a new quorum of echos for the
    /// given hash.
    fn check_new_echo_quorum(&mut self, round_id: RoundId, hash: C::Hash) -> bool {
        if self.rounds.contains_key(&round_id)
            && self.rounds[&round_id].outcome.quorum_echos.is_none()
            && self.is_quorum(self.rounds[&round_id].echos[&hash].keys().copied())
        {
            self.round_mut(round_id).outcome.quorum_echos = Some(hash);
            return true;
        }
        false
    }

    /// Updates the round's outcome and returns `true` if there is a new quorum of votes with the
    /// given value.
    fn check_new_vote_quorum(&mut self, round_id: RoundId, vote: bool) -> bool {
        if self.rounds.contains_key(&round_id)
            && self.rounds[&round_id].outcome.quorum_votes.is_none()
            && self.is_quorum(self.rounds[&round_id].votes[&vote].keys_some())
        {
            self.round_mut(round_id).outcome.quorum_votes = Some(vote);
            return true;
        }
        false
    }

    /// Adds a signed message content to the state, but does not call `update` and does not detect
    /// faults.
    fn add_content(
        &mut self,
        round_id: RoundId,
        content: Content<C>,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) {
        match content {
            Content::Proposal(proposal) => {
                if self
                    .round_mut(round_id)
                    .insert_proposal(proposal, signature)
                {
                    self.register_activity(validator_idx);
                    self.mark_dirty(round_id);
                }
            }
            Content::Echo(hash) => {
                if !self.faults.contains_key(&validator_idx)
                    && self
                        .round_mut(round_id)
                        .insert_echo(hash, validator_idx, signature)
                {
                    self.register_activity(validator_idx);
                    if self.check_new_echo_quorum(round_id, hash) {
                        self.mark_dirty(round_id);
                    }
                }
            }
            Content::Vote(vote) => {
                if !self.faults.contains_key(&validator_idx)
                    & self
                        .round_mut(round_id)
                        .insert_vote(vote, validator_idx, signature)
                {
                    self.register_activity(validator_idx);
                    if self.check_new_vote_quorum(round_id, vote) {
                        self.mark_dirty(round_id);
                    }
                }
            }
        }
    }

    /// Update the state after a new siganture from a validator was received.
    fn register_activity(&mut self, validator_idx: ValidatorIndex) {
        self.progress_detected = true;
        if !self.active[validator_idx] {
            self.active[validator_idx] = true;
            // If a validator is seen for the first time a proposal in any round that doesn't
            // consider them inactive could become accepted now.
            self.mark_dirty(self.first_non_finalized_round_id);
        }
    }

    /// If there is a signature for conflicting content, returns the content and signature.
    fn detect_fault(
        &self,
        round_id: RoundId,
        validator_idx: ValidatorIndex,
        content: &Content<C>,
    ) -> Option<(Content<C>, C::Signature)> {
        let round = self.round(round_id)?;
        match content {
            Content::Proposal(proposal) => {
                if validator_idx != self.leader(round_id) {
                    return None;
                }
                let hash = proposal.hash(); // TODO: Avoid redundant hashing!
                round
                    .proposals
                    .iter()
                    .find(|(hash2, _)| **hash2 != hash)
                    .map(|(_, (proposal2, sig))| (Content::Proposal(proposal2.clone()), *sig))
            }
            Content::Echo(hash) => round.echos.iter().find_map(|(hash2, echo_map)| {
                if hash2 == hash {
                    return None;
                }
                echo_map
                    .get(&validator_idx)
                    .map(|sig| (Content::Echo(*hash2), *sig))
            }),
            Content::Vote(vote) => {
                round.votes[&!vote][validator_idx].map(|sig| (Content::Vote(!vote), sig))
            }
        }
    }

    /// Updates the state and sends appropriate messages after a signature has been added to a
    /// round.
    fn update(&mut self, now: Timestamp) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        if self.maybe_dirty_round_id.is_none() || self.finalized_switch_block() {
            return outcomes; // This era has ended or all rounds are up to date.
        }
        while let Some(round_id) = self.maybe_dirty_round_id {
            self.create_round(round_id);
            if let Some(hash) = self.rounds[&round_id].proposals.keys().next().copied() {
                outcomes.extend(self.create_message(round_id, Content::Echo(hash)));
            }
            outcomes.extend(self.update_accepted_proposal(round_id, now));

            if self.has_accepted_proposal(round_id) {
                outcomes.extend(self.create_message(round_id, Content::Vote(true)));

                if self.is_committed_round(round_id) {
                    self.proposal_timeout = self.params.min_round_length();
                    outcomes.extend(self.finalize_round(round_id)); // Proposal is finalized!
                }

                // Proposed descendants of this block can now be validated.
                if let Some(proposals) = self.proposals_waiting_for_parent.remove(&round_id) {
                    let ancestor_values = self
                        .ancestor_values(round_id)
                        .expect("missing ancestors of accepted proposal");
                    for (proposal, rounds_and_senders) in proposals {
                        for (proposal_round_id, sender, signature) in rounds_and_senders {
                            outcomes.extend(self.validate_proposal(
                                proposal_round_id,
                                proposal.clone(),
                                ancestor_values.clone(),
                                sender,
                                signature,
                            ));
                        }
                    }
                }
            }
            if round_id == self.current_round {
                if now >= self.current_timeout {
                    outcomes.extend(self.create_message(round_id, Content::Vote(false)));
                }
                if self.is_skippable_round(round_id) || self.has_accepted_proposal(round_id) {
                    self.current_timeout = Timestamp::from(u64::MAX);
                    self.current_round = self.current_round.saturating_add(1);
                } else if let Some((maybe_parent_round_id, timestamp)) =
                    self.suitable_parent_round(now)
                {
                    if now < timestamp {
                        // The first opportunity to make a proposal is in the future; check again at
                        // that time.
                        outcomes.push(ProtocolOutcome::ScheduleTimer(timestamp, TIMER_ID_UPDATE));
                    } else if self.current_timeout > now + self.proposal_timeout {
                        // A proposal could be made now. Start the timer and propose if leader.
                        self.current_timeout = now + self.proposal_timeout;
                        self.proposal_timeout = self.proposal_timeout * 2u64;
                        outcomes.push(ProtocolOutcome::ScheduleTimer(
                            self.current_timeout,
                            TIMER_ID_UPDATE,
                        ));
                        outcomes.extend(self.propose_if_leader(maybe_parent_round_id, now));
                    }
                } else {
                    error!("No suitable parent for current round");
                }
            }
            self.maybe_dirty_round_id = round_id
                .checked_add(1)
                .filter(|r_id| *r_id <= self.current_round);
        }
        outcomes
    }

    /// Checks whether a proposal is accepted in that round, and adds it to the round outcome.
    fn update_accepted_proposal(
        &mut self,
        round_id: RoundId,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        if self.has_accepted_proposal(round_id) {
            return vec![]; // We already have an accepted proposal.
        }
        let proposal = if let Some((proposal, _)) = self
            .rounds
            .get(&round_id)
            .and_then(|round| round.proposals.get(&round.outcome.quorum_echos?))
        {
            proposal
        } else {
            return vec![]; // We don't have a proposal with a quorum of echos
        };
        if let Some(inactive) = &proposal.inactive {
            for idx in self.weights.keys() {
                if !inactive.contains(&idx) && !self.active[idx] {
                    // The proposal claims validator idx is active but we haven't seen anything from
                    // them yet.
                    return vec![];
                }
            }
        }
        let (first_skipped_round_id, rel_height) =
            if let Some(parent_round_id) = proposal.maybe_parent_round_id {
                if let Some(parent_height) = self
                    .round(parent_round_id)
                    .and_then(|round| round.outcome.accepted_proposal_height)
                {
                    (
                        parent_round_id.saturating_add(1),
                        parent_height.saturating_add(1),
                    )
                } else {
                    return vec![]; // Parent is not accepted yet.
                }
            } else {
                (0, 0)
            };
        if (first_skipped_round_id..round_id)
            .any(|skipped_round_id| !self.is_skippable_round(skipped_round_id))
        {
            return vec![]; // A skipped round is not skippable yet.
        }
        let min_child_timestamp = proposal
            .timestamp
            .saturating_add(self.params.min_round_length());

        // We have a proposal with accepted parent, a quorum of Echos, and all rounds since the
        // parent are skippable. That means the proposal is now accepted.
        self.round_mut(round_id).outcome.accepted_proposal_height = Some(rel_height);
        // Schedule an update where we check if this proposal's child can now be proposed.
        if now < min_child_timestamp {
            // Schedule an update where we check if this proposal's child can now be proposed.
            vec![ProtocolOutcome::ScheduleTimer(
                min_child_timestamp,
                TIMER_ID_UPDATE,
            )]
        } else {
            vec![]
        }
    }

    /// Sends a proposal to the `BlockValidator` component for validation. If no validation is
    /// needed, immediately calls `add_content`.
    fn validate_proposal(
        &mut self,
        round_id: RoundId,
        proposal: Proposal<C>,
        ancestor_values: Vec<C::ConsensusValue>,
        sender: NodeId,
        signature: C::Signature,
    ) -> ProtocolOutcomes<C> {
        if let Some((_, parent_proposal)) = proposal
            .maybe_parent_round_id
            .and_then(|parent_round_id| self.accepted_proposal(parent_round_id))
        {
            let min_round_len = self.params.min_round_length();
            if proposal.timestamp < parent_proposal.timestamp.saturating_add(min_round_len) {
                info!("rejecting proposal with timestamp earlier than the parent");
                return vec![];
            }
            if let (Some(inactive), Some(parent_inactive)) =
                (&proposal.inactive, &parent_proposal.inactive)
            {
                if !inactive.is_subset(parent_inactive) {
                    info!("rejecting proposal with more inactive validators than parent");
                    return vec![];
                }
            }
        }
        let validator_idx = self.leader(round_id);
        if let Some(block) = proposal
            .maybe_block
            .clone()
            .filter(ConsensusValueT::needs_validation)
        {
            self.log_proposal(&proposal, validator_idx, "requesting proposal validation");
            let block_context = BlockContext::new(proposal.timestamp, ancestor_values);
            let proposed_block = ProposedBlock::new(block, block_context);
            if self
                .proposals_waiting_for_validation
                .entry(proposed_block.clone())
                .or_default()
                .insert((round_id, proposal, sender, signature))
            {
                vec![ProtocolOutcome::ValidateConsensusValue {
                    sender,
                    proposed_block,
                }]
            } else {
                vec![] // Proposal was already known.
            }
        } else {
            if proposal.timestamp < self.params.start_timestamp() {
                info!("rejecting proposal with timestamp earlier than era start");
                return vec![];
            }
            self.log_proposal(
                &proposal,
                validator_idx,
                "proposal does not need validation",
            );
            let content = Content::Proposal(proposal);
            self.add_content(round_id, content, validator_idx, signature);
            vec![]
        }
    }

    /// Finalizes the round, notifying the rest of the node of the finalized block
    /// if it contained one.
    fn finalize_round(&mut self, round_id: RoundId) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        if round_id < self.first_non_finalized_round_id {
            return outcomes; // This round was already finalized.
        }
        let (relative_height, proposal) = if let Some((height, proposal)) =
            self.round(round_id).and_then(Round::accepted_proposal)
        {
            (height, proposal.clone())
        } else {
            error!(round_id, "missing finalized proposal; this is a bug");
            return outcomes;
        };
        if let Some(parent_round_id) = proposal.maybe_parent_round_id {
            // Output the parent first if it isn't already finalized.
            outcomes.extend(self.finalize_round(parent_round_id));
        }
        self.first_non_finalized_round_id = round_id.saturating_add(1);
        let value = if let Some(block) = proposal.maybe_block.clone() {
            block
        } else {
            return outcomes; // This era's last block is already finalized.
        };
        let proposer = self
            .validators
            .id(self.leader(round_id))
            .expect("validator not found")
            .clone();
        let terminal_block_data = self.accepted_switch_block(round_id).then(|| {
            let inactive_validators = proposal
                .inactive
                .iter()
                .flatten()
                .filter_map(|idx| self.validators.id(*idx))
                .cloned()
                .collect();
            TerminalBlockData {
                rewards: self
                    .validators
                    .iter()
                    .map(|v| (v.id().clone(), v.weight().0))
                    .collect(), // TODO
                inactive_validators,
            }
        });
        let finalized_block = FinalizedBlock {
            value,
            timestamp: proposal.timestamp,
            relative_height,
            // Faulty validators are already reported to the era supervisor via
            // validators_with_evidence.
            // TODO: Is this field entirely obsoleted by accusations?
            equivocators: vec![],
            terminal_block_data,
            proposer,
        };
        outcomes.push(ProtocolOutcome::FinalizedBlock(finalized_block));
        outcomes
    }

    fn propose_if_leader(
        &mut self,
        maybe_parent_round_id: Option<RoundId>,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let our_idx = if let Some((our_idx, _)) = self.active_validator {
            our_idx
        } else {
            return vec![]; // Not a validator.
        };
        if self.pending_proposal.is_some() // TODO: Or if it's in an old round?
            || !self.round_mut(self.current_round).proposals.is_empty()
            || our_idx != self.leader(self.current_round)
        {
            return vec![]; // We already proposed, or we are not the leader.
        }
        let (maybe_parent_round_id, ancestor_values) = match maybe_parent_round_id {
            Some(parent_round_id) => {
                if self.accepted_switch_block(parent_round_id)
                    || self.accepted_dummy_proposal(parent_round_id)
                {
                    let content = Content::Proposal(Proposal::dummy(now, parent_round_id));
                    let mut outcomes = self.create_message(self.current_round, content);
                    outcomes.extend(self.update(now));
                    return outcomes;
                }
                let ancestor_values = self
                    .ancestor_values(parent_round_id)
                    .expect("missing ancestor value");
                (Some(parent_round_id), ancestor_values)
            }
            None => (None, vec![]),
        };
        let block_context = BlockContext::new(now, ancestor_values);
        self.pending_proposal = Some((
            block_context.clone(),
            self.current_round,
            maybe_parent_round_id,
        ));
        vec![ProtocolOutcome::CreateNewBlock(block_context)]
    }

    /// Returns a parent if a block with that parent could be proposed in the current round, and the
    /// earliest possible timestamp for a new proposal.
    fn suitable_parent_round(&self, now: Timestamp) -> Option<(Option<RoundId>, Timestamp)> {
        let min_round_len = self.params.min_round_length();
        let mut maybe_parent = None;
        // We iterate through the rounds before the current one, in reverse order.
        for round_id in (0..self.current_round).rev() {
            if let Some((_, parent)) = self.accepted_proposal(round_id) {
                // All rounds higher than this one are skippable. When the accepted proposal's
                // timestamp is old enough it can be used as a parent.
                let timestamp = parent.timestamp.saturating_add(min_round_len);
                if now >= timestamp {
                    return Some((Some(round_id), timestamp));
                }
                if maybe_parent.map_or(true, |(_, timestamp2)| timestamp2 > timestamp) {
                    maybe_parent = Some((Some(round_id), timestamp));
                }
            }
            if !self.is_skippable_round(round_id) {
                return maybe_parent;
            }
        }
        // All rounds are skippable. When the era starts block 0 can be proposed.
        Some((None, self.params.start_timestamp()))
    }

    /// Returns whether a quorum has voted for `false`.
    fn is_skippable_round(&self, round_id: RoundId) -> bool {
        self.rounds
            .get(&round_id)
            .map_or(false, |round| round.outcome.quorum_votes == Some(false))
    }

    /// Returns whether a quorum has voted for `true`.
    fn is_committed_round(&self, round_id: RoundId) -> bool {
        self.rounds
            .get(&round_id)
            .map_or(false, |round| round.outcome.quorum_votes == Some(true))
    }

    /// Returns whether a round has an accepted proposal.
    fn has_accepted_proposal(&self, round_id: RoundId) -> bool {
        self.round(round_id).map_or(false, |round| {
            round.outcome.accepted_proposal_height.is_some()
        })
    }

    /// Returns the accepted proposal, if any, together with its height.
    fn accepted_proposal(&self, round_id: RoundId) -> Option<(u64, &Proposal<C>)> {
        self.round(round_id)?.accepted_proposal()
    }

    /// Returns `true` if the given validators, together will all faulty validators, form a quorum.
    fn is_quorum(&self, vidxs: impl Iterator<Item = ValidatorIndex>) -> bool {
        let mut sum = self.faulty_weight();
        let quorum_threshold = self.quorum_threshold();
        if sum > quorum_threshold {
            return true;
        }
        for vidx in vidxs {
            if !self.faults.contains_key(&vidx) {
                sum += self.weights[vidx];
                if sum > quorum_threshold {
                    return true;
                }
            }
        }
        false
    }

    /// Returns the accepted value from the given round and all its ancestors, or `None` if there is
    /// no accepted value in that round yet.
    fn ancestor_values(&self, mut round_id: RoundId) -> Option<Vec<C::ConsensusValue>> {
        let mut ancestor_values = vec![];
        loop {
            let (_, proposal) = self.accepted_proposal(round_id)?;
            ancestor_values.extend(proposal.maybe_block.clone());
            match proposal.maybe_parent_round_id {
                None => return Some(ancestor_values),
                Some(parent_round_id) => round_id = parent_round_id,
            }
        }
    }

    /// Returns the greatest weight such that two sets of validators with this weight can
    /// intersect in only faulty validators, i.e. have an intersection of weight `<= ftt`. A
    /// _quorum_ is any set with a weight strictly greater than this, so any two quora have at least
    /// one correct validator in common.
    fn quorum_threshold(&self) -> Weight {
        let total_weight = self.validators.total_weight().0;
        let ftt = self.ftt.0;
        #[allow(clippy::integer_arithmetic)] // Cannot overflow, even if both are u64::MAX.
        Weight(total_weight / 2 + ftt / 2 + (total_weight & ftt & 1))
    }

    /// Returns the total weight of validators known to be faulty.
    fn faulty_weight(&self) -> Weight {
        self.sum_weights(self.faults.keys())
    }

    /// Returns the sum of the weights of the given validators.
    fn sum_weights<'a>(&self, vidxs: impl Iterator<Item = &'a ValidatorIndex>) -> Weight {
        vidxs.map(|vidx| self.weights[*vidx]).sum()
    }

    /// Retrieves a shared reference to the round.
    fn round(&self, round_id: RoundId) -> Option<&Round<C>> {
        self.rounds.get(&round_id)
    }

    /// Retrieves a mutable reference to the round.
    /// If the round doesn't exist yet, it creates an empty one.
    fn round_mut(&mut self, round_id: RoundId) -> &mut Round<C> {
        match self.rounds.entry(round_id) {
            btree_map::Entry::Occupied(entry) => entry.into_mut(),
            btree_map::Entry::Vacant(entry) => entry.insert(Round::new(self.weights.len())),
        }
    }

    /// Creates a round if it doesn't exist yet.
    fn create_round(&mut self, round_id: RoundId) {
        self.round_mut(round_id); // This creates a round as a side effect.
    }

    /// Marks a round as dirty so that the next `upgrade` call will reevaluate it.
    fn mark_dirty(&mut self, round_id: RoundId) {
        if round_id <= self.current_round
            && self
                .maybe_dirty_round_id
                .map_or(true, |r_id| r_id > round_id)
        {
            self.maybe_dirty_round_id = Some(round_id);
        }
    }
}

/// A proposal in the consensus protocol.
#[derive(Clone, Hash, Serialize, Deserialize, Debug, PartialEq, Eq, DataSize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Proposal<C>
where
    C: Context,
{
    timestamp: Timestamp,
    maybe_block: Option<C::ConsensusValue>,
    maybe_parent_round_id: Option<RoundId>,
    /// The set of validators that appear to be inactive in this era.
    /// This is `None` in round 0 and in dummy blocks.
    inactive: Option<BTreeSet<ValidatorIndex>>,
}

impl<C: Context> Proposal<C> {
    fn dummy(timestamp: Timestamp, parent_round_id: RoundId) -> Self {
        Proposal {
            timestamp,
            maybe_block: None,
            maybe_parent_round_id: Some(parent_round_id),
            inactive: None,
        }
    }

    fn with_block(
        proposed_block: &ProposedBlock<C>,
        maybe_parent_round_id: Option<RoundId>,
        inactive: impl Iterator<Item = ValidatorIndex>,
    ) -> Self {
        Proposal {
            maybe_block: Some(proposed_block.value().clone()),
            timestamp: proposed_block.context().timestamp(),
            maybe_parent_round_id,
            inactive: maybe_parent_round_id.map(|_| inactive.collect()),
        }
    }

    fn hash(&self) -> C::Hash {
        let serialized = bincode::serialize(&self).expect("failed to serialize fields");
        <C as Context>::hash(&serialized)
    }
}

/// The content of a message in the main protocol, as opposed to the
/// sync messages, which are somewhat decoupled from the rest of the
/// protocol. This message, along with the instance and round ID,
/// are what are signed by the active validators.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Content<C: Context> {
    Proposal(Proposal<C>),
    Echo(C::Hash),
    Vote(bool),
}

impl<C: Context> Content<C> {
    fn is_proposal(&self) -> bool {
        matches!(self, Content::Proposal(_))
    }
}

/// Indicates the outcome of a given round.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct RoundOutcome<C>
where
    C: Context,
{
    /// This is `Some(h)` if there is an accepted proposal with relative height `h`, i.e. there is
    /// a quorum of echos, `h` accepted ancestors, and all rounds since the parent's are skippable.
    accepted_proposal_height: Option<u64>,
    quorum_echos: Option<C::Hash>,
    quorum_votes: Option<bool>,
}

impl<C: Context> Default for RoundOutcome<C> {
    fn default() -> RoundOutcome<C> {
        RoundOutcome {
            accepted_proposal_height: None,
            quorum_echos: None,
            quorum_votes: None,
        }
    }
}

/// All messages of the protocol.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Message<C: Context> {
    /// Partial information about the sender's protocol state. The receiver should send missing
    /// data.
    ///
    /// The sender chooses a random peer and a random era, and includes in its `SyncState` message
    /// information about received proposals, echos and votes. The idea is to set the `i`-th bit in
    /// the `u128` fields to `1` if we have a signature from the `i`-th validator.
    ///
    /// To keep the size of these messages constant even if there are more than 128 validators, a
    /// random interval is selected and only information about validators in that interval is
    /// included: The bit with the lowest significance corresponds to validator number
    /// `first_validator_idx`, and the one with the highest to
    /// `(first_validator_idx + 127) % validator_count`.
    ///
    /// For example if there are 500 validators and `first_validator_idx` is 450, the `u128`'s bits
    /// refer to validators 450, 451, ..., 499, 0, 1, ..., 77.
    SyncState {
        /// The round the information refers to.
        round_id: RoundId,
        /// The proposal hash with the most echos (by weight).
        proposal_hash: Option<C::Hash>,
        /// Whether the sender has the proposal with that hash.
        proposal: bool,
        /// The index of the first validator covered by the bit fields below.
        first_validator_idx: ValidatorIndex,
        /// A bit field with 1 for every validator the sender has an echo from.
        echos: u128,
        /// A bit field with 1 for every validator the sender has a `true` vote from.
        true_votes: u128,
        /// A bit field with 1 for every validator the sender has a `false` vote from.
        false_votes: u128,
        /// A bit field with 1 for every validator the sender has evidence against.
        faulty: u128,
        instance_id: C::InstanceId,
    },
    Signed {
        round_id: RoundId,
        instance_id: C::InstanceId,
        content: Content<C>,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    },
}

impl<C: Context> Message<C> {
    fn new_empty_round_sync_state(
        round_id: RoundId,
        first_validator_idx: ValidatorIndex,
        faulty: u128,
        instance_id: C::InstanceId,
    ) -> Self {
        Message::SyncState {
            round_id,
            proposal_hash: None,
            proposal: false,
            first_validator_idx,
            echos: 0,
            true_votes: 0,
            false_votes: 0,
            faulty,
            instance_id,
        }
    }

    fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).expect("should serialize message")
    }

    fn instance_id(&self) -> &C::InstanceId {
        match self {
            Message::SyncState { instance_id, .. } | Message::Signed { instance_id, .. } => {
                instance_id
            }
        }
    }
}

impl<C> ConsensusProtocol<C> for SimpleConsensus<C>
where
    C: Context + 'static,
{
    fn handle_message(
        &mut self,
        _rng: &mut NodeRng,
        sender: NodeId,
        msg: Vec<u8>,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        match bincode::deserialize::<Message<C>>(msg.as_slice()) {
            Err(err) => {
                let outcome = ProtocolOutcome::InvalidIncomingMessage(msg, sender, err.into());
                vec![outcome]
            }
            Ok(message) if *message.instance_id() != self.instance_id => {
                let instance_id = message.instance_id();
                info!(?instance_id, ?sender, "wrong instance ID; disconnecting");
                let err = anyhow::Error::msg("invalid instance ID");
                let outcome = ProtocolOutcome::InvalidIncomingMessage(msg.clone(), sender, err);
                vec![outcome]
            }
            Ok(Message::SyncState {
                round_id,
                proposal_hash,
                proposal,
                first_validator_idx,
                echos,
                true_votes,
                false_votes,
                faulty,
                instance_id: _,
            }) => self.handle_sync_state(
                round_id,
                proposal_hash,
                proposal,
                first_validator_idx,
                echos,
                true_votes,
                false_votes,
                faulty,
                sender,
            ),
            Ok(Message::Signed {
                round_id,
                instance_id: _,
                content,
                validator_idx,
                signature,
            }) => self.handle_signed_message(
                msg,
                round_id,
                content,
                validator_idx,
                signature,
                sender,
                now,
            ),
        }
    }

    /// Handles the firing of various timers in the protocol.
    fn handle_timer(
        &mut self,
        now: Timestamp,
        timer_id: TimerId,
        rng: &mut NodeRng,
    ) -> ProtocolOutcomes<C> {
        match timer_id {
            TIMER_ID_SYNC_PEER => self.handle_sync_peer_timer(now, rng),
            TIMER_ID_UPDATE => {
                self.mark_dirty(self.current_round);
                self.update(now)
            }
            TIMER_ID_LOG_PARTICIPATION => {
                self.log_participation();
                match self.config.log_participation_interval {
                    Some(interval) if !self.evidence_only && !self.finalized_switch_block() => {
                        vec![ProtocolOutcome::ScheduleTimer(now + interval, timer_id)]
                    }
                    _ => vec![],
                }
            }
            TIMER_ID_STANDSTILL_ALERT => self.handle_standstill_alert_timer(now),
            // TIMER_ID_VERTEX_WITH_FUTURE_TIMESTAMP => {
            //     self.synchronizer.add_past_due_stored_vertices(now)
            // }
            _ => unreachable!("unexpected timer ID"),
        }
    }

    fn handle_is_current(&self, now: Timestamp) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        // TODO: In this protocol the interval should be shorter than in Highway.
        if let Some(interval) = self.config.request_state_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()) + interval / 100,
                TIMER_ID_SYNC_PEER,
            ));
        }
        if let Some(interval) = self.config.log_participation_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()) + interval,
                TIMER_ID_LOG_PARTICIPATION,
            ));
        }
        if let Some(timeout) = self.config.standstill_timeout {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()) + timeout,
                TIMER_ID_STANDSTILL_ALERT,
            ));
        }
        outcomes
    }

    fn handle_action(&mut self, action_id: ActionId, now: Timestamp) -> ProtocolOutcomes<C> {
        error!(?action_id, %now, "unexpected action");
        vec![]
    }

    fn propose(&mut self, proposed_block: ProposedBlock<C>, now: Timestamp) -> ProtocolOutcomes<C> {
        if let Some((block_context, proposal_round_id, maybe_parent_round_id)) =
            self.pending_proposal.take()
        {
            if block_context != *proposed_block.context() {
                warn!(block_context = ?proposed_block.context(), "skipping outdated proposal");
                self.pending_proposal =
                    Some((block_context, proposal_round_id, maybe_parent_round_id));
                return vec![];
            }
            let inactive = self
                .weights
                .keys()
                .filter(|idx| !self.active[*idx] && !self.faults.contains_key(idx));
            let content = Content::Proposal(Proposal::with_block(
                &proposed_block,
                maybe_parent_round_id,
                inactive,
            ));
            let mut outcomes = self.create_message(proposal_round_id, content);
            outcomes.extend(self.update(now));
            outcomes
        } else {
            error!("unexpected call to propose");
            vec![]
        }
    }

    fn resolve_validity(
        &mut self,
        proposed_block: ProposedBlock<C>,
        valid: bool,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let rounds_and_node_ids = self
            .proposals_waiting_for_validation
            .remove(&proposed_block)
            .into_iter()
            .flatten();
        if valid {
            let mut outcomes = vec![];
            for (round_id, proposal, _sender, signature) in rounds_and_node_ids {
                info!(?proposal, "handling valid proposal");
                let content = Content::Proposal(proposal);
                let validator_idx = self.leader(round_id);
                self.add_content(round_id, content, validator_idx, signature);
                outcomes.extend(self.update(now));
            }
            outcomes
        } else {
            for (round_id, _, sender, _) in rounds_and_node_ids {
                // We don't disconnect from the faulty sender here: The block validator considers
                // the value "invalid" even if it just couldn't download the deploys, which could
                // just be because the original sender went offline.
                let validator_index = self.leader(round_id).0;
                info!(validator_index, %round_id, ?sender, "dropping invalid proposal");
            }
            vec![]
        }
    }

    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        now: Timestamp,
        _unit_hash_file: Option<PathBuf>,
    ) -> ProtocolOutcomes<C> {
        // TODO: Use the unit hash file to remember at least all our own messages from at least all
        // rounds that aren't finalized (ideally with finality signatures) yet. To support the whole
        // internet restarting, we'd need to store all our own messages.
        if let Some(our_idx) = self.validators.get_index(&our_id) {
            self.active_validator = Some((our_idx, secret));
            self.active[our_idx] = true;
            return vec![ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()),
                TIMER_ID_UPDATE,
            )];
        } else {
            warn!(
                ?our_id,
                "we are not a validator in this era; not activating"
            );
        }
        vec![]
    }

    fn deactivate_validator(&mut self) {
        self.active_validator = None;
    }

    fn set_evidence_only(&mut self) {
        self.evidence_only = true;
        self.rounds.clear();
        self.proposals_waiting_for_parent.clear();
        self.proposals_waiting_for_validation.clear();
    }

    fn has_evidence(&self, vid: &C::ValidatorId) -> bool {
        self.validators
            .get_index(vid)
            .and_then(|idx| self.faults.get(&idx))
            .map_or(false, Fault::is_direct)
    }

    fn mark_faulty(&mut self, vid: &C::ValidatorId) {
        if let Some(idx) = self.validators.get_index(vid) {
            self.faults.entry(idx).or_insert(Fault::Indirect);
        }
    }

    fn request_evidence(&self, peer: NodeId, vid: &C::ValidatorId) -> ProtocolOutcomes<C> {
        if let Some(v_idx) = self.validators.get_index(vid) {
            // Send the peer a sync message, so they will send us evidence we are missing.
            let round_id = self.current_round;
            let payload = self.create_sync_state_message(v_idx, round_id).serialize();
            vec![ProtocolOutcome::CreatedTargetedMessage(payload, peer)]
        } else {
            error!(?vid, "unknown validator ID");
            vec![]
        }
    }

    fn set_paused(&mut self, paused: bool, now: Timestamp) -> ProtocolOutcomes<C> {
        if self.paused && !paused {
            self.paused = paused;
            self.mark_dirty(self.current_round);
            self.update(now)
        } else {
            self.paused = paused;
            vec![]
        }
    }

    fn validators_with_evidence(&self) -> Vec<&C::ValidatorId> {
        self.faults
            .iter()
            .filter(|(_, fault)| fault.is_direct())
            .filter_map(|(vidx, _)| self.validators.id(*vidx))
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn is_active(&self) -> bool {
        self.active_validator.is_some()
    }

    fn instance_id(&self) -> &C::InstanceId {
        &self.instance_id
    }

    fn next_round_length(&self) -> Option<TimeDiff> {
        Some(self.params.min_round_length())
    }
}

/// A reason for a validator to be marked as faulty.
///
/// The `Banned` state is fixed from the beginning and can't be replaced. However, `Indirect` can
/// be replaced with `Direct` evidence, which has the same effect but doesn't rely on information
/// from other consensus protocol instances.
#[derive(DataSize, Debug)]
pub(crate) enum Fault<C>
where
    C: Context,
{
    /// The validator was known to be malicious from the beginning. All their messages are
    /// considered invalid in this Highway instance.
    Banned,
    /// We have direct evidence of the validator's fault.
    // TODO: Store only the necessary information, e.g. not the full signed proposal, and only one
    // round ID, instance ID and validator index.
    Direct(Message<C>, Message<C>),
    /// The validator is known to be faulty, but the evidence is not in this era.
    Indirect,
}

impl<C: Context> Fault<C> {
    fn is_direct(&self) -> bool {
        matches!(self, Fault::Direct(_, _))
    }

    fn is_banned(&self) -> bool {
        matches!(self, Fault::Banned)
    }
}

/// A validator's participation status: whether they are faulty or inactive.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
enum ParticipationStatus {
    LastSeenInRound(RoundId),
    Inactive,
    EquivocatedInOtherEra,
    Equivocated,
}

/// A map of status (faulty, inactive) by validator ID.
#[derive(Debug)]
// False positive, as the fields of this struct are all used in logging validator participation.
#[allow(dead_code)]
pub(crate) struct Participation<C>
where
    C: Context,
{
    instance_id: C::InstanceId,
    faulty_stake_percent: u8,
    inactive_stake_percent: u8,
    inactive_validators: Vec<(ValidatorIndex, C::ValidatorId, ParticipationStatus)>,
    faulty_validators: Vec<(ValidatorIndex, C::ValidatorId, ParticipationStatus)>,
}

impl ParticipationStatus {
    /// Returns a `Status` for a validator unless they are honest and online.
    fn for_index<C: Context + 'static>(
        idx: ValidatorIndex,
        sc: &SimpleConsensus<C>,
    ) -> Option<ParticipationStatus> {
        if let Some(fault) = sc.faults.get(&idx) {
            return Some(match fault {
                Fault::Banned | Fault::Indirect => ParticipationStatus::EquivocatedInOtherEra,
                Fault::Direct(_, _) => ParticipationStatus::Equivocated,
            });
        }
        // TODO: Avoid iterating over all old rounds every time we log this.
        for r_id in sc.rounds.keys().rev() {
            if sc.has_echoed(*r_id, idx)
                || sc.has_voted(*r_id, idx)
                || (sc.has_accepted_proposal(*r_id) && sc.leader(*r_id) == idx)
            {
                if r_id.saturating_add(2) < sc.current_round {
                    return Some(ParticipationStatus::LastSeenInRound(*r_id));
                } else {
                    return None; // Seen recently; considered currently active.
                }
            }
        }
        Some(ParticipationStatus::Inactive)
    }
}
