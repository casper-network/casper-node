//! # The Zug consensus protocol.
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
//! A `SyncRequest` message containing information about a random part of the local protocol state
//! is periodically sent to a random peer. The peer compares that to its local state, and responds
//! with all signed messages that it has and the other is missing.

pub(crate) mod config;
mod fault;
mod message;
mod params;
mod participation;
mod proposal;
mod round;
#[cfg(test)]
mod tests;
mod wal;

use std::{
    any::Any,
    cmp::Reverse,
    collections::{btree_map, BTreeMap, HashMap, HashSet},
    fmt::Debug,
    iter,
    path::PathBuf,
};

use datasize::DataSize;
use either::Either;
use itertools::Itertools;
use rand::{seq::IteratorRandom, Rng};
use tracing::{debug, error, event, info, warn, Level};

use casper_types::{system::auction::BLOCK_REWARD, TimeDiff, Timestamp, U512};

use crate::{
    components::consensus::{
        config::Config,
        consensus_protocol::{
            BlockContext, ConsensusProtocol, FinalizedBlock, ProposedBlock, ProtocolOutcome,
            ProtocolOutcomes, TerminalBlockData,
        },
        protocols,
        traits::{ConsensusValueT, Context},
        utils::{ValidatorIndex, ValidatorMap, Validators, Weight},
        ActionId, EraMessage, EraRequest, LeaderSequence, TimerId,
    },
    types::{Chainspec, NodeId},
    utils, NodeRng,
};
use fault::Fault;
use message::{Content, SignedMessage, SyncResponse};
use params::Params;
use participation::{Participation, ParticipationStatus};
use proposal::{HashedProposal, Proposal};
use round::Round;
use wal::{Entry, ReadWal, WriteWal};

pub(crate) use message::{Message, SyncRequest};

/// The timer for syncing with a random peer.
const TIMER_ID_SYNC_PEER: TimerId = TimerId(0);
/// The timer for calling `update`.
const TIMER_ID_UPDATE: TimerId = TimerId(1);
/// The timer for logging inactive validators.
const TIMER_ID_LOG_PARTICIPATION: TimerId = TimerId(2);

/// The maximum number of future rounds we instantiate if we get messages from rounds that we
/// haven't started yet.
const MAX_FUTURE_ROUNDS: u32 = 7200; // Don't drop messages in 2-hour eras with 1-second rounds.

/// Identifies a single [`Round`] in the protocol.
pub(crate) type RoundId = u32;

type ProposalsAwaitingParent = HashSet<(RoundId, NodeId)>;
type ProposalsAwaitingValidation<C> = HashSet<(RoundId, HashedProposal<C>, NodeId)>;

/// Contains the portion of the state required for an active validator to participate in the
/// protocol.
#[derive(DataSize)]
pub(crate) struct ActiveValidator<C>
where
    C: Context,
{
    idx: ValidatorIndex,
    secret: C::ValidatorSecret,
}

impl<C: Context> Debug for ActiveValidator<C> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActiveValidator")
            .field("idx", &self.idx)
            .field("secret", &"<REDACTED>")
            .finish()
    }
}

/// Contains the state required for the protocol.
#[derive(Debug, DataSize)]
pub(crate) struct Zug<C>
where
    C: Context,
{
    /// Contains numerical parameters for the protocol
    params: Params<C>,
    /// The timeout for the current round's proposal, in milliseconds
    proposal_timeout_millis: f64,
    /// The validators in this instantiation of the protocol
    validators: Validators<C::ValidatorId>,
    /// If we are a validator ourselves, we must know which index we
    /// are in the [`Validators`] and have a private key for consensus.
    active_validator: Option<ActiveValidator<C>>,
    /// When an era has already completed, sometimes we still need to keep
    /// it around to provide evidence for equivocation in previous eras.
    evidence_only: bool,
    /// Proposals which have not yet had their parent accepted, by parent round ID.
    proposals_waiting_for_parent:
        HashMap<RoundId, HashMap<HashedProposal<C>, ProposalsAwaitingParent>>,
    /// Incoming blocks we can't add yet because we are waiting for validation.
    proposals_waiting_for_validation: HashMap<ProposedBlock<C>, ProposalsAwaitingValidation<C>>,
    /// If we requested a new block from the block proposer component this contains the proposal's
    /// round ID and the parent's round ID, if there is a parent.
    pending_proposal: Option<(BlockContext<C>, RoundId, Option<RoundId>)>,
    leader_sequence: LeaderSequence,
    /// The [`Round`]s of this protocol which we've instantiated.
    rounds: BTreeMap<RoundId, Round<C>>,
    /// List of faulty validators and their type of fault.
    faults: HashMap<ValidatorIndex, Fault<C>>,
    /// The configuration for the protocol
    config: config::Config,
    /// This is a signed message for every validator we have received a signature from.
    active: ValidatorMap<Option<SignedMessage<C>>>,
    /// The lowest round ID of a block that could still be finalized in the future.
    first_non_finalized_round_id: RoundId,
    /// The lowest round that needs to be considered in `upgrade`.
    maybe_dirty_round_id: Option<RoundId>,
    /// The lowest non-skippable round without an accepted value.
    current_round: RoundId,
    /// The time when the current round started.
    current_round_start: Timestamp,
    /// Whether anything was recently added to the protocol state.
    progress_detected: bool,
    /// Whether or not the protocol is currently paused
    paused: bool,
    /// The next update we have set a timer for. This helps deduplicate redundant calls to
    /// `update`.
    next_scheduled_update: Timestamp,
    /// The write-ahead log to prevent honest nodes from double-signing upon restart.
    write_wal: Option<WriteWal<C>>,
    /// The rewards based on the finalized rounds so far.
    rewards: BTreeMap<C::ValidatorId, u64>,
}

impl<C: Context + 'static> Zug<C> {
    /// Creates a new [`Zug`] instance.
    #[allow(clippy::too_many_arguments)]
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
    ) -> Zug<C> {
        let validators = protocols::common::validators::<C>(faulty, inactive, validator_stakes);
        let weights = protocols::common::validator_weights::<C>(&validators);
        let active: ValidatorMap<_> = weights.iter().map(|_| None).collect();
        let core_config = &chainspec.core_config;

        // Use the estimate from the previous era as the proposal timeout. Start with one minimum
        // timeout times the grace period factor: This is what we would settle on if proposals
        // always got accepted exactly after one minimum timeout.
        let proposal_timeout_millis = prev_cp
            .and_then(|cp| cp.as_any().downcast_ref::<Zug<C>>())
            .map(|zug| zug.proposal_timeout_millis)
            .unwrap_or_else(|| {
                config.zug.proposal_timeout.millis() as f64
                    * (config.zug.proposal_grace_period as f64 / 100.0 + 1.0)
            });

        let mut can_propose: ValidatorMap<bool> = weights.iter().map(|_| true).collect();
        for vidx in validators.iter_cannot_propose_idx() {
            can_propose[vidx] = false;
        }
        let faults: HashMap<_, _> = validators
            .iter_banned_idx()
            .map(|idx| (idx, Fault::Banned))
            .collect();
        let leader_sequence = LeaderSequence::new(seed, &weights, can_propose);

        info!(
            %instance_id, %era_start_time, %proposal_timeout_millis,
            "initializing Zug instance",
        );

        let params = Params::new(
            instance_id,
            core_config.minimum_block_time,
            era_start_time,
            core_config.minimum_era_height,
            era_start_time + core_config.era_duration,
            protocols::common::ftt::<C>(core_config.finality_threshold_fraction, &validators),
        );

        let rewards = validators.iter().map(|v| (v.id().clone(), 0)).collect();

        Zug {
            leader_sequence,
            proposals_waiting_for_parent: HashMap::new(),
            proposals_waiting_for_validation: HashMap::new(),
            rounds: BTreeMap::new(),
            first_non_finalized_round_id: 0,
            maybe_dirty_round_id: None,
            current_round: 0,
            current_round_start: Timestamp::MAX,
            evidence_only: false,
            faults,
            active,
            config: config.zug.clone(),
            params,
            proposal_timeout_millis,
            validators,
            active_validator: None,
            pending_proposal: None,
            progress_detected: false,
            paused: false,
            next_scheduled_update: Timestamp::MAX,
            write_wal: None,
            rewards,
        }
    }

    /// Creates a new boxed [`Zug`] instance.
    #[allow(clippy::too_many_arguments)]
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
        wal_file: PathBuf,
    ) -> (Box<dyn ConsensusProtocol<C>>, ProtocolOutcomes<C>) {
        let mut zug = Self::new(
            instance_id,
            validator_stakes,
            faulty,
            inactive,
            chainspec,
            config,
            prev_cp,
            era_start_time,
            seed,
        );

        let outcomes = zug.open_wal(wal_file, now);

        (Box::new(zug), outcomes)
    }

    /// Prints a log statement listing the inactive and faulty validators.
    fn log_participation(&self) {
        let mut inactive_w: u64 = 0;
        let mut faulty_w: u64 = 0;
        let total_w = self.validators.total_weight().0;
        let mut inactive_validators = Vec::new();
        let mut faulty_validators = Vec::new();
        for (idx, v_id) in self.validators.enumerate_ids() {
            if let Some(status) = ParticipationStatus::for_index(idx, self) {
                match status {
                    ParticipationStatus::Equivocated
                    | ParticipationStatus::EquivocatedInOtherEra => {
                        faulty_w = faulty_w.saturating_add(self.validators.weight(idx).0);
                        faulty_validators.push((idx, v_id.clone(), status));
                    }
                    ParticipationStatus::Inactive | ParticipationStatus::LastSeenInRound(_) => {
                        inactive_w = inactive_w.saturating_add(self.validators.weight(idx).0);
                        inactive_validators.push((idx, v_id.clone(), status));
                    }
                }
            }
        }
        inactive_validators.sort_by_key(|(idx, _, status)| (Reverse(*status), *idx));
        faulty_validators.sort_by_key(|(idx, _, status)| (Reverse(*status), *idx));
        let inactive_w_100 = u128::from(inactive_w).saturating_mul(100);
        let faulty_w_100 = u128::from(faulty_w).saturating_mul(100);
        let participation = Participation::<C> {
            instance_id: *self.instance_id(),
            inactive_stake_percent: utils::div_round(inactive_w_100, u128::from(total_w)) as u8,
            faulty_stake_percent: utils::div_round(faulty_w_100, u128::from(total_w)) as u8,
            inactive_validators,
            faulty_validators,
        };
        info!(?participation, "validator participation");
    }

    /// Returns whether the switch block has already been finalized.
    fn finalized_switch_block(&self) -> bool {
        if let Some(round_id) = self.first_non_finalized_round_id.checked_sub(1) {
            self.accepted_switch_block(round_id) || self.accepted_dummy_proposal(round_id)
        } else {
            false
        }
    }

    /// Returns whether a block was accepted that, if finalized, would be the last one.
    fn accepted_switch_block(&self, round_id: RoundId) -> bool {
        match self.round(round_id).and_then(Round::accepted_proposal) {
            None => false,
            Some((height, proposal)) => {
                proposal.maybe_block().is_some() // not a dummy proposal
                    && height.saturating_add(1) >= self.params.end_height() // reached era height
                    && proposal.timestamp() >= self.params.end_timestamp() // minimum era duration
            }
        }
    }

    /// Returns whether a proposal without a block was accepted, i.e. whether some ancestor of the
    /// accepted proposal is a switch block.
    fn accepted_dummy_proposal(&self, round_id: RoundId) -> bool {
        match self.round(round_id).and_then(Round::accepted_proposal) {
            None => false,
            Some((_, proposal)) => proposal.maybe_block().is_none(),
        }
    }

    /// Returns whether the validator has already sent an `Echo` in this round.
    fn has_echoed(&self, round_id: RoundId, validator_idx: ValidatorIndex) -> bool {
        self.round(round_id)
            .map_or(false, |round| round.has_echoed(validator_idx))
    }

    /// Returns whether the validator has already cast a `true` or `false` vote.
    fn has_voted(&self, round_id: RoundId, validator_idx: ValidatorIndex) -> bool {
        self.round(round_id)
            .map_or(false, |round| round.has_voted(validator_idx))
    }

    /// Request the latest state from a random peer.
    fn handle_sync_peer_timer(&self, now: Timestamp, rng: &mut NodeRng) -> ProtocolOutcomes<C> {
        if self.evidence_only || self.finalized_switch_block() {
            return vec![]; // Era has ended. No further progress is expected.
        }
        debug!(
            instance_id = ?self.instance_id(),
            "syncing with random peer",
        );
        // Inform a peer about our protocol state and schedule the next request.
        let first_validator_idx = ValidatorIndex(rng.gen_range(0..self.validators.len() as u32));
        let round_id = (self.first_non_finalized_round_id..=self.current_round)
            .choose(rng)
            .unwrap_or(self.current_round);
        let payload = self.create_sync_request(first_validator_idx, round_id);
        let mut outcomes = vec![ProtocolOutcome::CreatedRequestToRandomPeer(payload.into())];
        // Periodically sync the state with a random peer.
        if let Some(interval) = self.config.sync_state_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now + interval,
                TIMER_ID_SYNC_PEER,
            ));
        }
        outcomes
    }

    /// Prints a log message if the message is a proposal.
    fn log_proposal(&self, proposal: &HashedProposal<C>, round_id: RoundId, msg: &str) {
        let creator_index = self.leader(round_id);
        let creator = if let Some(creator) = self.validators.id(creator_index) {
            creator
        } else {
            error!(?creator_index, ?round_id, "{}: invalid creator", msg);
            return;
        };
        info!(
            hash = %proposal.hash(),
            %creator,
            creator_index = creator_index.0,
            round_id,
            timestamp = %proposal.timestamp(),
            "{}", msg,
        );
    }

    /// Creates a `SyncRequest` message to inform a peer about our view of the given round, so that
    /// the peer can send us any data we are missing.
    ///
    /// If there are more than 128 validators, the information only covers echoes and votes of
    /// validators with index in `first_validator_idx..=(first_validator_idx + 127)`.
    fn create_sync_request(
        &self,
        first_validator_idx: ValidatorIndex,
        round_id: RoundId,
    ) -> SyncRequest<C> {
        let faulty = self.validator_bit_field(first_validator_idx, self.faults.keys().cloned());
        let active = self.validator_bit_field(first_validator_idx, self.active.keys_some());
        let round = match self.round(round_id) {
            Some(round) => round,
            None => {
                return SyncRequest::new_empty_round(
                    round_id,
                    first_validator_idx,
                    faulty,
                    active,
                    *self.instance_id(),
                );
            }
        };
        let true_votes =
            self.validator_bit_field(first_validator_idx, round.votes(true).keys_some());
        let false_votes =
            self.validator_bit_field(first_validator_idx, round.votes(false).keys_some());
        // We only request information about the proposal with the most echoes, by weight.
        // TODO: If there's no quorum, should we prefer the one for which we have the leader's echo?
        let proposal_hash = round.quorum_echoes().or_else(|| {
            round
                .echoes()
                .iter()
                .max_by_key(|(_, echo_map)| self.sum_weights(echo_map.keys()))
                .map(|(hash, _)| *hash)
        });
        let has_proposal = round.proposal().map(HashedProposal::hash) == proposal_hash.as_ref();
        let mut echoes = 0;
        if let Some(echo_map) = proposal_hash.and_then(|hash| round.echoes().get(&hash)) {
            echoes = self.validator_bit_field(first_validator_idx, echo_map.keys().cloned());
        }
        SyncRequest {
            round_id,
            proposal_hash,
            has_proposal,
            first_validator_idx,
            echoes,
            true_votes,
            false_votes,
            active,
            faulty,
            instance_id: *self.instance_id(),
        }
    }

    /// Returns a bit field where each bit stands for a validator: the least significant one for
    /// `first_idx` and the most significant one for `fist_idx + 127`, wrapping around at the total
    /// number of validators. The bits of the validators in `index_iter` that fall into that
    /// range are set to `1`, the others are `0`.
    fn validator_bit_field(
        &self,
        ValidatorIndex(first_idx): ValidatorIndex,
        index_iter: impl Iterator<Item = ValidatorIndex>,
    ) -> u128 {
        let validator_count = self.validators.len() as u32;
        if first_idx >= validator_count {
            return 0;
        }
        let mut bit_field: u128 = 0;
        for ValidatorIndex(v_idx) in index_iter {
            // The validator's bit is v_idx - first_idx, but we wrap around.
            let idx = match v_idx.overflowing_sub(first_idx) {
                (idx, false) => idx,
                // An underflow occurred. Add validator_count to wrap back around.
                (idx, true) => idx.wrapping_add(validator_count),
            };
            if idx < u128::BITS {
                bit_field |= 1_u128.wrapping_shl(idx); // Set bit number i to 1.
            }
        }
        bit_field
    }

    /// Returns an iterator over all validator indexes whose bits in the `bit_field` are `1`, where
    /// the least significant one stands for `first_idx` and the most significant one for
    /// `first_idx + 127`, wrapping around.
    fn iter_validator_bit_field(
        &self,
        ValidatorIndex(mut idx): ValidatorIndex,
        mut bit_field: u128,
    ) -> impl Iterator<Item = ValidatorIndex> {
        let validator_count = self.validators.len() as u32;
        iter::from_fn(move || {
            if bit_field == 0 || idx >= validator_count {
                return None; // No remaining bits with value 1.
            }
            let zeros = bit_field.trailing_zeros();
            // The index of the validator whose bit is 1. We shift the bits to the right so that the
            // least significant bit now corresponds to this one, then we output the index and set
            // the bit to 0.
            bit_field = bit_field.wrapping_shr(zeros);
            bit_field &= !1;
            idx = match idx.overflowing_add(zeros) {
                (i, false) => i,
                // If an overflow occurs, go back via an underflow, so the value modulo
                // validator_count is correct again.
                (i, true) => i
                    .checked_rem(validator_count)?
                    .wrapping_sub(validator_count),
            }
            .checked_rem(validator_count)?;
            Some(ValidatorIndex(idx))
        })
    }

    /// Returns whether `v_idx` is covered by a validator index that starts at `first_idx`.
    fn validator_bit_field_includes(
        &self,
        ValidatorIndex(first_idx): ValidatorIndex,
        ValidatorIndex(v_idx): ValidatorIndex,
    ) -> bool {
        let validator_count = self.validators.len() as u32;
        if first_idx >= validator_count {
            return false;
        }
        let high_bit = u128::BITS.saturating_sub(1);
        // The overflow bit is the 33rd bit of the actual sum.
        let (last_idx, last_idx_overflow) = first_idx.overflowing_add(high_bit);
        if v_idx >= first_idx {
            // v_idx is at least first_idx, so it's in the range unless it's higher than the last
            // index, taking into account its 33rd bit.
            last_idx_overflow || v_idx <= last_idx
        } else {
            // v_idx is less than first_idx. But if going from the first to the last index we wrap
            // around, we might still arrive at v_idx:
            let (v_idx2, v_idx2_overflow) = v_idx.overflowing_add(validator_count);
            if v_idx2_overflow == last_idx_overflow {
                v_idx2 <= last_idx
            } else {
                last_idx_overflow
            }
        }
    }

    /// Returns the leader in the specified round.
    pub(crate) fn leader(&self, round_id: RoundId) -> ValidatorIndex {
        if let Some(round) = self.round(round_id) {
            return round.leader();
        }
        self.leader_sequence.leader(u64::from(round_id))
    }

    /// If we are an active validator and it would be safe for us to sign this message and we
    /// haven't signed it before, we sign it, add it to our state and gossip it to the network.
    ///
    /// Does not call `update`!
    fn create_message(&mut self, round_id: RoundId, content: Content<C>) -> ProtocolOutcomes<C> {
        let (validator_idx, secret_key) = if let Some(active_validator) = &self.active_validator {
            (active_validator.idx, &active_validator.secret)
        } else {
            return vec![];
        };
        if self.paused {
            return vec![];
        }
        let already_signed = match &content {
            Content::Echo(_) => self.has_echoed(round_id, validator_idx),
            Content::Vote(_) => self.has_voted(round_id, validator_idx),
        };
        if already_signed {
            return vec![]; // Not creating message, so we don't double-sign or duplicate.
        }
        let signed_msg = SignedMessage::sign_new(
            round_id,
            *self.instance_id(),
            content,
            validator_idx,
            secret_key,
        );
        // We only add and send the new message if we are able to record it. If that fails we
        // wouldn't know about our own message after a restart and risk double-signing.
        if self.record_entry(&Entry::SignedMessage(signed_msg.clone()))
            && self.add_content(signed_msg.clone())
        {
            let message = Message::Signed(signed_msg);
            vec![ProtocolOutcome::CreatedGossipMessage(message.into())]
        } else {
            vec![]
        }
    }

    /// When we receive evidence for a fault, we must notify the rest of the network of this
    /// evidence. Beyond that, we can remove all of the faulty validator's previous information
    /// from the protocol state.
    fn handle_fault(
        &mut self,
        signed_msg: SignedMessage<C>,
        validator_id: C::ValidatorId,
        content2: Content<C>,
        signature2: C::Signature,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        self.record_entry(&Entry::Evidence(
            signed_msg.clone(),
            content2.clone(),
            signature2,
        ));
        self.handle_fault_no_wal(signed_msg, validator_id, content2, signature2, now)
    }

    /// Internal to handle_fault, documentation from that applies
    fn handle_fault_no_wal(
        &mut self,
        signed_msg: SignedMessage<C>,
        validator_id: C::ValidatorId,
        content2: Content<C>,
        signature2: C::Signature,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let validator_idx = signed_msg.validator_idx;
        warn!(?signed_msg, ?content2, id = %validator_id, "validator double-signed");
        let fault = Fault::Direct(signed_msg, content2, signature2);
        self.faults.insert(validator_idx, fault);
        if Some(validator_idx) == self.active_validator.as_ref().map(|av| av.idx) {
            error!(our_idx = validator_idx.0, "we are faulty; deactivating");
            self.active_validator = None;
        }
        self.active[validator_idx] = None;
        self.progress_detected = true;
        let mut outcomes = vec![ProtocolOutcome::NewEvidence(validator_id)];
        if self.faulty_weight() > self.params.ftt() {
            outcomes.push(ProtocolOutcome::FttExceeded);
            return outcomes;
        }

        // Remove all votes and echoes from the faulty validator: They count towards every quorum
        // now so nobody has to store their messages.
        for round in self.rounds.values_mut() {
            round.remove_votes_and_echoes(validator_idx);
        }

        // Recompute quorums; if any new quorums are found, call `update`.
        for round_id in
            self.first_non_finalized_round_id..=self.rounds.keys().last().copied().unwrap_or(0)
        {
            if !self.rounds.contains_key(&round_id) {
                continue;
            }
            if self.rounds[&round_id].quorum_echoes().is_none() {
                let hashes = self.rounds[&round_id]
                    .echoes()
                    .keys()
                    .copied()
                    .collect_vec();
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
    fn handle_sync_request(
        &self,
        sync_request: SyncRequest<C>,
        sender: NodeId,
    ) -> (ProtocolOutcomes<C>, Option<EraMessage<C>>) {
        let SyncRequest {
            round_id,
            mut proposal_hash,
            mut has_proposal,
            first_validator_idx,
            mut echoes,
            true_votes,
            false_votes,
            active,
            faulty,
            instance_id,
        } = sync_request;
        if first_validator_idx.0 >= self.validators.len() as u32 {
            info!(
                first_validator_idx = first_validator_idx.0,
                %sender,
                "invalid SyncRequest message"
            );
            return (vec![ProtocolOutcome::Disconnect(sender)], None);
        }

        // If we don't have that round we have no information the requester is missing.
        let round = match self.round(round_id) {
            Some(round) => round,
            None => return (vec![], None),
        };

        // If the peer has no or a wrong proposal we assume they don't have any echoes for the
        // correct one. We don't send them the right proposal, though: they might already have it.
        if round.quorum_echoes() != proposal_hash && round.quorum_echoes().is_some() {
            has_proposal = true;
            echoes = 0;
            proposal_hash = round.quorum_echoes();
        }

        // The bit field of validators we know to be faulty.
        let our_faulty = self.validator_bit_field(first_validator_idx, self.faults.keys().cloned());
        // The echo signatures and proposal/hash we will send in the response.
        let mut proposal_or_hash = None;
        let mut echo_sigs = BTreeMap::new();
        // The bit field of validators we have echoes from in this round.
        let mut our_echoes: u128 = 0;

        if let Some(hash) = proposal_hash {
            if let Some(echo_map) = round.echoes().get(&hash) {
                // Send them echoes they are missing, but exclude faulty validators.
                our_echoes =
                    self.validator_bit_field(first_validator_idx, echo_map.keys().cloned());
                let missing_echoes = our_echoes & !(echoes | faulty | our_faulty);
                for v_idx in self.iter_validator_bit_field(first_validator_idx, missing_echoes) {
                    echo_sigs.insert(v_idx, echo_map[&v_idx]);
                }
                if has_proposal {
                    proposal_or_hash = Some(Either::Right(hash));
                } else {
                    // If they don't have the proposal make sure we include the leader's echo.
                    let leader_idx = round.leader();
                    if !self.validator_bit_field_includes(first_validator_idx, leader_idx) {
                        if let Some(signature) = echo_map.get(&leader_idx) {
                            echo_sigs.insert(leader_idx, *signature);
                        }
                    }
                    if let Some(proposal) = round.proposal() {
                        if *proposal.hash() == hash {
                            proposal_or_hash = Some(Either::Left(proposal.inner().clone()));
                        }
                    }
                }
            }
        }

        // Send them votes they are missing, but exclude faulty validators. If there already is a
        // quorum omit the votes that go against the quorum, since they are irrelevant.
        let our_true_votes: u128 = if round.quorum_votes() == Some(false) {
            0
        } else {
            self.validator_bit_field(first_validator_idx, round.votes(true).keys_some())
        };
        let missing_true_votes = our_true_votes & !(true_votes | faulty | our_faulty);
        let true_vote_sigs = self
            .iter_validator_bit_field(first_validator_idx, missing_true_votes)
            .map(|v_idx| (v_idx, round.votes(true)[v_idx].unwrap()))
            .collect();
        let our_false_votes: u128 = if round.quorum_votes() == Some(true) {
            0
        } else {
            self.validator_bit_field(first_validator_idx, round.votes(false).keys_some())
        };
        let missing_false_votes = our_false_votes & !(false_votes | faulty | our_faulty);
        let false_vote_sigs = self
            .iter_validator_bit_field(first_validator_idx, missing_false_votes)
            .map(|v_idx| (v_idx, round.votes(false)[v_idx].unwrap()))
            .collect();

        let mut outcomes = vec![];

        // Add evidence for validators they don't know are faulty.
        let missing_faulty = our_faulty & !faulty;
        let mut evidence = vec![];
        for v_idx in self.iter_validator_bit_field(first_validator_idx, missing_faulty) {
            match &self.faults[&v_idx] {
                Fault::Banned => {
                    info!(
                        validator_index = v_idx.0,
                        %sender,
                        "peer disagrees about banned validator; disconnecting"
                    );
                    return (vec![ProtocolOutcome::Disconnect(sender)], None);
                }
                Fault::Direct(signed_msg, content2, signature2) => {
                    evidence.push((signed_msg.clone(), content2.clone(), *signature2));
                }
                Fault::Indirect => {
                    let vid = self.validators.id(v_idx).unwrap().clone();
                    outcomes.push(ProtocolOutcome::SendEvidence(sender, vid));
                }
            }
        }

        // Send any signed messages that prove a validator is not completely inactive. We only
        // need to do this for validators that the requester doesn't know are active, and that
        // we haven't already included any signature from in our votes, echoes or evidence.
        let our_active = self.validator_bit_field(first_validator_idx, self.active.keys_some());
        let missing_active =
            our_active & !(active | our_echoes | our_true_votes | our_false_votes | our_faulty);
        let signed_messages = self
            .iter_validator_bit_field(first_validator_idx, missing_active)
            .filter_map(|v_idx| self.active[v_idx].clone())
            .collect();

        // Send the serialized sync response to the requester
        let sync_response = SyncResponse {
            round_id,
            proposal_or_hash,
            echo_sigs,
            true_vote_sigs,
            false_vote_sigs,
            signed_messages,
            evidence,
            instance_id,
        };
        (outcomes, Some(Message::SyncResponse(sync_response).into()))
    }

    /// The response containing the parts from the sender's protocol state that we were missing.
    fn handle_sync_response(
        &mut self,
        sync_response: SyncResponse<C>,
        sender: NodeId,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let SyncResponse {
            round_id,
            proposal_or_hash,
            echo_sigs,
            true_vote_sigs,
            false_vote_sigs,
            mut signed_messages,
            evidence,
            instance_id,
        } = sync_response;

        let (proposal_hash, proposal) = match proposal_or_hash {
            Some(Either::Left(proposal)) => {
                let hashed_prop = HashedProposal::new(proposal);
                (Some(*hashed_prop.hash()), Some(hashed_prop.into_inner()))
            }
            Some(Either::Right(hash)) => (Some(hash), None),
            None => (None, None),
        };

        // Reconstruct the signed messages from the echo and vote signatures, and add them to the
        // messages we need to handle.
        let mut contents = vec![];
        if let Some(hash) = proposal_hash {
            for (validator_idx, signature) in echo_sigs {
                contents.push((validator_idx, Content::Echo(hash), signature));
            }
        }
        for (validator_idx, signature) in true_vote_sigs {
            contents.push((validator_idx, Content::Vote(true), signature));
        }
        for (validator_idx, signature) in false_vote_sigs {
            contents.push((validator_idx, Content::Vote(false), signature));
        }
        signed_messages.extend(
            contents
                .into_iter()
                .map(|(validator_idx, content, signature)| SignedMessage {
                    round_id,
                    instance_id,
                    content,
                    validator_idx,
                    signature,
                }),
        );

        // Handle the signed messages, evidence and proposal. The proposal must be handled last,
        // since the other data may contain its justification, i.e. the proposer's own echo, or a
        // quorum of echoes.
        let mut outcomes = vec![];
        for signed_msg in signed_messages {
            outcomes.extend(self.handle_signed_message(signed_msg, sender, now));
        }
        for (signed_msg, content2, signature2) in evidence {
            outcomes.extend(self.handle_evidence(signed_msg, content2, signature2, sender, now));
        }
        if let Some(proposal) = proposal {
            outcomes.extend(self.handle_proposal(round_id, proposal, sender, now));
        }
        outcomes
    }

    /// The main entry point for signed echoes or votes. This function mostly authenticates
    /// and authorizes the message, passing it to [`add_content`] if it passes snuff for the
    /// main protocol logic.
    fn handle_signed_message(
        &mut self,
        signed_msg: SignedMessage<C>,
        sender: NodeId,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let validator_idx = signed_msg.validator_idx;
        let validator_id = if let Some(validator_id) = self.validators.id(validator_idx) {
            validator_id.clone()
        } else {
            warn!(
                ?signed_msg,
                %sender,
                "invalid incoming message: validator index out of range",
            );
            return vec![ProtocolOutcome::Disconnect(sender)];
        };

        if self.faults.contains_key(&validator_idx) {
            debug!(?validator_id, "ignoring message from faulty validator");
            return vec![];
        }

        if signed_msg.round_id > self.current_round.saturating_add(MAX_FUTURE_ROUNDS) {
            debug!(?signed_msg, "dropping message from future round");
            return vec![];
        }

        if self.evidence_only {
            debug!(?signed_msg, "received an irrelevant message");
            return vec![];
        }

        if let Some(round) = self.round(signed_msg.round_id) {
            if round.contains(&signed_msg.content, validator_idx) {
                debug!(?signed_msg, %sender, "received a duplicated message");
                return vec![];
            }
        }

        if !signed_msg.verify_signature(&validator_id) {
            warn!(?signed_msg, %sender, "invalid signature",);
            return vec![ProtocolOutcome::Disconnect(sender)];
        }

        if let Some((content2, signature2)) = self.detect_fault(&signed_msg) {
            let evidence_msg = Message::Evidence(signed_msg.clone(), content2.clone(), signature2);
            let mut outcomes =
                self.handle_fault(signed_msg, validator_id, content2, signature2, now);
            outcomes.push(ProtocolOutcome::CreatedGossipMessage(evidence_msg.into()));
            return outcomes;
        }

        if self.faults.contains_key(&signed_msg.validator_idx) {
            debug!(?signed_msg, "dropping message from faulty validator");
        } else {
            self.record_entry(&Entry::SignedMessage(signed_msg.clone()));
            if self.add_content(signed_msg) {
                return self.update(now);
            }
        }

        vec![]
    }

    /// Verifies an evidence message that is supposed to contain two conflicting sigantures by the
    /// same validator, and then calls `handle_fault`.
    fn handle_evidence(
        &mut self,
        signed_msg: SignedMessage<C>,
        content2: Content<C>,
        signature2: C::Signature,
        sender: NodeId,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let validator_idx = signed_msg.validator_idx;
        if let Some(Fault::Direct(..)) = self.faults.get(&validator_idx) {
            return vec![]; // Validator is already known to be faulty.
        }
        let validator_id = if let Some(validator_id) = self.validators.id(validator_idx) {
            validator_id.clone()
        } else {
            warn!(
                ?signed_msg,
                %sender,
                "invalid incoming evidence: validator index out of range",
            );
            return vec![ProtocolOutcome::Disconnect(sender)];
        };
        if !signed_msg.content.contradicts(&content2) {
            warn!(
                ?signed_msg,
                ?content2,
                %sender,
                "invalid evidence: contents don't conflict",
            );
            return vec![ProtocolOutcome::Disconnect(sender)];
        }
        if !signed_msg.verify_signature(&validator_id)
            || !signed_msg
                .with(content2.clone(), signature2)
                .verify_signature(&validator_id)
        {
            warn!(
                ?signed_msg,
                ?content2,
                %sender,
                "invalid signature in evidence",
            );
            return vec![ProtocolOutcome::Disconnect(sender)];
        }
        self.handle_fault(signed_msg, validator_id, content2, signature2, now)
    }

    /// Checks whether an incoming proposal should be added to the protocol state and starts
    /// validation.
    fn handle_proposal(
        &mut self,
        round_id: RoundId,
        proposal: Proposal<C>,
        sender: NodeId,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        let leader_idx = self.leader(round_id);

        macro_rules! log_proposal {
            ($lvl:expr, $prop:expr, $msg:expr $(,)?) => {
                event!(
                    $lvl,
                    round_id,
                    parent = $prop.maybe_parent_round_id,
                    timestamp = %$prop.timestamp,
                    leader_idx = leader_idx.0,
                    ?sender,
                    "{}",
                    $msg
                );
            }
        }

        if let Some(parent_round_id) = proposal.maybe_parent_round_id {
            if parent_round_id >= round_id {
                log_proposal!(
                    Level::WARN,
                    proposal,
                    "invalid proposal: parent is not from an earlier round",
                );
                return vec![ProtocolOutcome::Disconnect(sender)];
            }
        }

        if proposal.timestamp > now + self.config.clock_tolerance {
            log_proposal!(
                Level::TRACE,
                proposal,
                "received a proposal with a timestamp far in the future; dropping",
            );
            return vec![];
        }
        if proposal.timestamp > now {
            log_proposal!(
                Level::TRACE,
                proposal,
                "received a proposal with a timestamp slightly in the future",
            );
        }
        if (proposal.maybe_parent_round_id.is_none() || proposal.maybe_block.is_none())
            != proposal.inactive.is_none()
        {
            log_proposal!(
                Level::WARN,
                proposal,
                "invalid proposal: inactive must be present in all except the first and dummy proposals",
            );
            return vec![ProtocolOutcome::Disconnect(sender)];
        }
        if let Some(inactive) = &proposal.inactive {
            if inactive
                .iter()
                .any(|idx| *idx == leader_idx || self.validators.id(*idx).is_none())
            {
                log_proposal!(
                    Level::WARN,
                    proposal,
                    "invalid proposal: invalid inactive validator index",
                );
                return vec![ProtocolOutcome::Disconnect(sender)];
            }
        }

        let hashed_prop = HashedProposal::new(proposal);

        if self.round(round_id).map_or(true, |round| {
            !round.has_echoes_for_proposal(hashed_prop.hash())
        }) {
            log_proposal!(
                Level::DEBUG,
                hashed_prop.inner(),
                "dropping proposal: missing echoes"
            );
            return vec![];
        }

        if self.round(round_id).and_then(Round::proposal) == Some(&hashed_prop) {
            log_proposal!(
                Level::DEBUG,
                hashed_prop.inner(),
                "dropping proposal: we already have it"
            );
            return vec![];
        }

        let ancestor_values = if let Some(parent_round_id) = hashed_prop.maybe_parent_round_id() {
            if let Some(ancestor_values) = self.ancestor_values(parent_round_id) {
                ancestor_values
            } else {
                log_proposal!(
                    Level::DEBUG,
                    hashed_prop.inner(),
                    "storing proposal for later; still missing ancestors",
                );
                self.proposals_waiting_for_parent
                    .entry(parent_round_id)
                    .or_insert_with(HashMap::new)
                    .entry(hashed_prop)
                    .or_insert_with(HashSet::new)
                    .insert((round_id, sender));
                return vec![];
            }
        } else {
            vec![]
        };

        let mut outcomes = self.validate_proposal(round_id, hashed_prop, ancestor_values, sender);
        outcomes.extend(self.update(now));
        outcomes
    }

    /// Updates the round's outcome and returns `true` if there is a new quorum of echoes for the
    /// given hash.
    fn check_new_echo_quorum(&mut self, round_id: RoundId, hash: C::Hash) -> bool {
        if self.rounds.contains_key(&round_id)
            && self.rounds[&round_id].quorum_echoes().is_none()
            && self.is_quorum(self.rounds[&round_id].echoes()[&hash].keys().copied())
        {
            self.round_mut(round_id).set_quorum_echoes(hash);
            return true;
        }
        false
    }

    /// Updates the round's outcome and returns `true` if there is a new quorum of votes with the
    /// given value.
    fn check_new_vote_quorum(&mut self, round_id: RoundId, vote: bool) -> bool {
        if self.rounds.contains_key(&round_id)
            && self.rounds[&round_id].quorum_votes().is_none()
            && self.is_quorum(self.rounds[&round_id].votes(vote).keys_some())
        {
            self.round_mut(round_id).set_quorum_votes(vote);
            if !vote {
                info!(%round_id, "round is now skippable");
            } else if self.rounds[&round_id].accepted_proposal().is_none() {
                info!(%round_id, "round committed; no accepted proposal yet");
            }
            return true;
        }
        false
    }

    /// Adds a signed message to the WAL such that we can avoid double signing upon recovery if the
    /// node shuts down. Returns `true` if the message was added successfully.
    fn record_entry(&mut self, entry: &Entry<C>) -> bool {
        match self.write_wal.as_mut().map(|ww| ww.record_entry(entry)) {
            None => false,
            Some(Ok(())) => true,
            Some(Err(err)) => {
                self.active_validator = None;
                self.write_wal = None;
                error!(
                    %err,
                    "could not record a signed message to the WAL; deactivating"
                );
                false
            }
        }
    }

    /// Consumes all of the signed messages we've previously recorded in our write ahead log, and
    /// sets up the log for appending future messages. If it fails it prints an error log and
    /// the WAL remains `None`: That way we can still observe the protocol but not participate as
    /// a validator.
    pub(crate) fn open_wal(&mut self, wal_file: PathBuf, now: Timestamp) -> ProtocolOutcomes<C> {
        // Open the file for reading.
        let mut read_wal = match ReadWal::<C>::new(&wal_file) {
            Ok(read_wal) => read_wal,
            Err(err) => {
                error!(%err, "could not create a ReadWal using this file");
                return vec![];
            }
        };

        let mut outcomes = vec![];

        // Read all messages recorded in the file.
        loop {
            match read_wal.read_next_entry() {
                Ok(Some(next_entry)) => match next_entry {
                    Entry::SignedMessage(next_message) => {
                        if !self.add_content(next_message) {
                            error!("Could not add content from WAL.");
                            return outcomes;
                        }
                    }
                    Entry::Proposal(next_proposal, corresponding_round_id) => {
                        if self
                            .round(corresponding_round_id)
                            .and_then(Round::proposal)
                            .map(HashedProposal::inner)
                            == Some(&next_proposal)
                        {
                            warn!("Proposal from WAL is duplicated.");
                            continue;
                        }
                        let mut ancestor_values = vec![];
                        if let Some(mut round_id) = next_proposal.maybe_parent_round_id {
                            loop {
                                let proposal = if let Some(proposal) =
                                    self.round(round_id).and_then(Round::proposal)
                                {
                                    proposal
                                } else {
                                    error!("Proposal from WAL is missing ancestors.");
                                    return outcomes;
                                };
                                if self.round(round_id).and_then(Round::quorum_echoes)
                                    != Some(*proposal.hash())
                                {
                                    error!("Proposal from WAL has unaccepted ancestor.");
                                    return outcomes;
                                }
                                ancestor_values.extend(proposal.maybe_block().cloned());
                                match proposal.maybe_parent_round_id() {
                                    None => break,
                                    Some(parent_round_id) => round_id = parent_round_id,
                                }
                            }
                        }
                        if self
                            .round_mut(corresponding_round_id)
                            .insert_proposal(HashedProposal::new(next_proposal.clone()))
                        {
                            self.mark_dirty(corresponding_round_id);
                            if let Some(block) = next_proposal.maybe_block {
                                let block_context =
                                    BlockContext::new(next_proposal.timestamp, ancestor_values);
                                let proposed_block = ProposedBlock::new(block, block_context);
                                outcomes
                                    .push(ProtocolOutcome::HandledProposedBlock(proposed_block));
                            }
                        }
                    }
                    Entry::Evidence(
                        conflicting_message,
                        conflicting_message_content,
                        conflicting_signature,
                    ) => {
                        let validator_id = {
                            if let Some(validator_id) =
                                self.validators.id(conflicting_message.validator_idx)
                            {
                                validator_id.clone()
                            } else {
                                warn!(
                                    index = conflicting_message.validator_idx.0,
                                    "No validator present at this index, despite holding \
                                    conflicting messages for it in the WAL"
                                );
                                continue;
                            }
                        };
                        let new_outcomes = self.handle_fault_no_wal(
                            conflicting_message,
                            validator_id,
                            conflicting_message_content,
                            conflicting_signature,
                            now,
                        );
                        // Ignore most outcomes: These have been processed before the restart.
                        outcomes.extend(new_outcomes.into_iter().filter(|outcome| match outcome {
                            ProtocolOutcome::FttExceeded
                            | ProtocolOutcome::WeAreFaulty
                            | ProtocolOutcome::FinalizedBlock(_)
                            | ProtocolOutcome::ValidateConsensusValue { .. }
                            | ProtocolOutcome::HandledProposedBlock(..)
                            | ProtocolOutcome::NewEvidence(_) => true,
                            ProtocolOutcome::SendEvidence(_, _)
                            | ProtocolOutcome::CreatedGossipMessage(_)
                            | ProtocolOutcome::CreatedTargetedMessage(_, _)
                            | ProtocolOutcome::CreatedMessageToRandomPeer(_)
                            | ProtocolOutcome::CreatedTargetedRequest(_, _)
                            | ProtocolOutcome::CreatedRequestToRandomPeer(_)
                            | ProtocolOutcome::ScheduleTimer(_, _)
                            | ProtocolOutcome::QueueAction(_)
                            | ProtocolOutcome::CreateNewBlock(_)
                            | ProtocolOutcome::DoppelgangerDetected
                            | ProtocolOutcome::Disconnect(_) => false,
                        }));
                    }
                },
                Ok(None) => {
                    break;
                }
                Err(err) => {
                    error!(
                        ?err,
                        "couldn't read a message from the WAL: was this node recently shut down?"
                    );
                    return outcomes; // Not setting WAL file; won't actively participate.
                }
            }
        }

        // Open the file for appending.
        match WriteWal::new(&wal_file) {
            Ok(write_wal) => self.write_wal = Some(write_wal),
            Err(err) => error!(?err, ?wal_file, "could not create a WAL using this file"),
        }

        outcomes
    }

    /// Adds a signed message content to the state.
    /// Does not call `update` and does not detect faults.
    fn add_content(&mut self, signed_msg: SignedMessage<C>) -> bool {
        if self.active[signed_msg.validator_idx].is_none() {
            self.active[signed_msg.validator_idx] = Some(signed_msg.clone());
            // We considered this validator inactive until now, and didn't accept proposals that
            // didn't have them in the `inactive` field. Mark all relevant rounds as dirty so that
            // the next `update` call checks all proposals again.
            self.mark_dirty(self.first_non_finalized_round_id);
        }
        let SignedMessage {
            round_id,
            instance_id: _,
            content,
            validator_idx,
            signature,
        } = signed_msg;
        match content {
            Content::Echo(hash) => {
                if self
                    .round_mut(round_id)
                    .insert_echo(hash, validator_idx, signature)
                {
                    debug!(round_id, %hash, validator = validator_idx.0, "inserted echo");
                    self.progress_detected = true;
                    if self.check_new_echo_quorum(round_id, hash) {
                        self.mark_dirty(round_id);
                    }
                    return true;
                }
            }
            Content::Vote(vote) => {
                if self
                    .round_mut(round_id)
                    .insert_vote(vote, validator_idx, signature)
                {
                    debug!(round_id, vote, validator = validator_idx.0, "inserted vote");
                    self.progress_detected = true;
                    if self.check_new_vote_quorum(round_id, vote) {
                        self.mark_dirty(round_id);
                    }
                    return true;
                }
            }
        }
        false
    }

    /// If there is a signature for conflicting content, returns the content and signature.
    fn detect_fault(&self, signed_msg: &SignedMessage<C>) -> Option<(Content<C>, C::Signature)> {
        let round = self.round(signed_msg.round_id)?;
        match &signed_msg.content {
            Content::Echo(hash) => round.echoes().iter().find_map(|(hash2, echo_map)| {
                if hash2 == hash {
                    return None;
                }
                echo_map
                    .get(&signed_msg.validator_idx)
                    .map(|sig| (Content::Echo(*hash2), *sig))
            }),
            Content::Vote(vote) => {
                round.votes(!vote)[signed_msg.validator_idx].map(|sig| (Content::Vote(!vote), sig))
            }
        }
    }

    /// Sets an update timer for the given timestamp, unless an earlier timer is already set.
    fn schedule_update(&mut self, timestamp: Timestamp) -> ProtocolOutcomes<C> {
        if self.next_scheduled_update > timestamp {
            self.next_scheduled_update = timestamp;
            vec![ProtocolOutcome::ScheduleTimer(timestamp, TIMER_ID_UPDATE)]
        } else {
            vec![]
        }
    }

    /// Updates the state and sends appropriate messages after a signature has been added to a
    /// round.
    fn update(&mut self, now: Timestamp) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        if self.finalized_switch_block() || self.faulty_weight() > self.params.ftt() {
            return outcomes; // This era has ended or the FTT was exceeded.
        }
        if let Some(dirty_round_id) = self.maybe_dirty_round_id {
            for round_id in dirty_round_id.. {
                outcomes.extend(self.update_round(round_id, now));
                if round_id >= self.current_round {
                    break;
                }
            }
        }
        self.maybe_dirty_round_id = None;
        outcomes
    }

    /// Updates a round and sends appropriate messages.
    fn update_round(&mut self, round_id: RoundId, now: Timestamp) -> ProtocolOutcomes<C> {
        self.create_round(round_id);
        let mut outcomes = vec![];

        // If we have a proposal, echo it.
        if let Some(&hash) = self.rounds[&round_id].proposal().map(HashedProposal::hash) {
            outcomes.extend(self.create_message(round_id, Content::Echo(hash)));
        }

        // Update the round outcome if there is a new accepted proposal.
        if self.update_accepted_proposal(round_id) {
            if round_id == self.current_round {
                self.update_proposal_timeout(now);
            }
            // Vote for finalizing this proposal.
            outcomes.extend(self.create_message(round_id, Content::Vote(true)));
            // Proposed descendants of this proposal can now be validated.
            if let Some(proposals) = self.proposals_waiting_for_parent.remove(&round_id) {
                let ancestor_values = self
                    .ancestor_values(round_id)
                    .expect("missing ancestors of accepted proposal");
                for (proposal, rounds_and_senders) in proposals {
                    for (proposal_round_id, sender) in rounds_and_senders {
                        outcomes.extend(self.validate_proposal(
                            proposal_round_id,
                            proposal.clone(),
                            ancestor_values.clone(),
                            sender,
                        ));
                    }
                }
            }
        }

        if round_id == self.current_round {
            let current_timeout = self
                .current_round_start
                .saturating_add(self.proposal_timeout());
            if now >= current_timeout {
                outcomes.extend(self.create_message(round_id, Content::Vote(false)));
                self.update_proposal_timeout(now);
            } else if self.faults.contains_key(&self.leader(round_id)) {
                outcomes.extend(self.create_message(round_id, Content::Vote(false)));
            }
            if self.is_skippable_round(round_id) || self.has_accepted_proposal(round_id) {
                self.current_round_start = Timestamp::MAX;
                self.current_round = self.current_round.saturating_add(1);
                info!(
                    round_id = self.current_round,
                    leader = self.leader(self.current_round).0,
                    "started a new round"
                );
            } else if let Some((maybe_parent_round_id, timestamp)) = self.suitable_parent_round(now)
            {
                if now < timestamp {
                    // The first opportunity to make a proposal is in the future; check again at
                    // that time.
                    outcomes.extend(self.schedule_update(timestamp));
                } else {
                    if self.current_round_start > now {
                        // A proposal could be made now. Start the timer and propose if leader.
                        self.current_round_start = now;
                        outcomes.extend(self.propose_if_leader(maybe_parent_round_id, now));
                    }
                    let current_timeout = self
                        .current_round_start
                        .saturating_add(self.proposal_timeout());
                    if current_timeout > now {
                        outcomes.extend(self.schedule_update(current_timeout));
                    }
                }
            } else {
                error!("No suitable parent for current round");
            }
        }

        // If the round has an accepted proposal and is committed, it is finalized.
        if self.has_accepted_proposal(round_id) && self.is_committed_round(round_id) {
            outcomes.extend(self.finalize_round(round_id));
        }
        outcomes
    }

    /// If a new proposal is accepted in that round, adds it to the round outcome and returns
    /// `true`.
    fn update_accepted_proposal(&mut self, round_id: RoundId) -> bool {
        if self.has_accepted_proposal(round_id) {
            return false; // We already have an accepted proposal.
        }
        let proposal = if let Some(proposal) = self.round(round_id).and_then(Round::proposal) {
            proposal
        } else {
            return false; // We don't have a proposal.
        };
        if self.round(round_id).and_then(Round::quorum_echoes) != Some(*proposal.hash()) {
            return false; // We don't have a quorum of echoes.
        }
        if let Some(inactive) = proposal.inactive() {
            for (idx, _) in self.validators.enumerate_ids() {
                if !inactive.contains(&idx)
                    && self.active[idx].is_none()
                    && !self.faults.contains_key(&idx)
                {
                    // The proposal claims validator idx is active but we haven't seen anything from
                    // them yet.
                    return false;
                }
            }
        }
        let (first_skipped_round_id, rel_height) =
            if let Some(parent_round_id) = proposal.maybe_parent_round_id() {
                if let Some((parent_height, _)) = self
                    .round(parent_round_id)
                    .and_then(Round::accepted_proposal)
                {
                    (
                        parent_round_id.saturating_add(1),
                        parent_height.saturating_add(1),
                    )
                } else {
                    return false; // Parent is not accepted yet.
                }
            } else {
                (0, 0)
            };
        if (first_skipped_round_id..round_id)
            .any(|skipped_round_id| !self.is_skippable_round(skipped_round_id))
        {
            return false; // A skipped round is not skippable yet.
        }

        // We have a proposal with accepted parent, a quorum of echoes, and all rounds since the
        // parent are skippable. That means the proposal is now accepted.
        self.round_mut(round_id)
            .set_accepted_proposal_height(rel_height);
        true
    }

    /// Sends a proposal to the `BlockValidator` component for validation. If no validation is
    /// needed, immediately calls `insert_proposal`.
    fn validate_proposal(
        &mut self,
        round_id: RoundId,
        proposal: HashedProposal<C>,
        ancestor_values: Vec<C::ConsensusValue>,
        sender: NodeId,
    ) -> ProtocolOutcomes<C> {
        if proposal.timestamp() < self.params.start_timestamp() {
            info!("rejecting proposal with timestamp earlier than era start");
            return vec![];
        }
        if let Some((_, parent_proposal)) = proposal
            .maybe_parent_round_id()
            .and_then(|parent_round_id| self.accepted_proposal(parent_round_id))
        {
            let min_block_time = self.params.min_block_time();
            if proposal.timestamp() < parent_proposal.timestamp().saturating_add(min_block_time) {
                info!("rejecting proposal with timestamp earlier than the parent");
                return vec![];
            }
            if let (Some(inactive), Some(parent_inactive)) =
                (proposal.inactive(), parent_proposal.inactive())
            {
                if !inactive.is_subset(parent_inactive) {
                    info!("rejecting proposal with more inactive validators than parent");
                    return vec![];
                }
            }
        }
        let block_context = BlockContext::new(proposal.timestamp(), ancestor_values);
        if let Some(block) = proposal
            .maybe_block()
            .filter(|value| value.needs_validation())
            .cloned()
        {
            self.log_proposal(&proposal, round_id, "requesting proposal validation");
            let proposed_block = ProposedBlock::new(block, block_context);
            if self
                .proposals_waiting_for_validation
                .entry(proposed_block.clone())
                .or_default()
                .insert((round_id, proposal, sender))
            {
                return vec![ProtocolOutcome::ValidateConsensusValue {
                    sender,
                    proposed_block,
                }];
            }
        } else {
            self.log_proposal(&proposal, round_id, "proposal does not need validation");
            if self.round_mut(round_id).insert_proposal(proposal.clone()) {
                self.record_entry(&Entry::Proposal(proposal.inner().clone(), round_id));
                self.progress_detected = true;
                self.mark_dirty(round_id);
                if let Some(block) = proposal.maybe_block().cloned() {
                    let proposed_block = ProposedBlock::new(block, block_context);
                    return vec![ProtocolOutcome::HandledProposedBlock(proposed_block)];
                }
            }
        }
        vec![] // Proposal was already known.
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
        if let Some(parent_round_id) = proposal.maybe_parent_round_id() {
            // Output the parent first if it isn't already finalized.
            outcomes.extend(self.finalize_round(parent_round_id));
        }
        for prune_round_id in self.first_non_finalized_round_id..round_id {
            info!(round_id = prune_round_id, "skipped round");
            self.round_mut(prune_round_id).prune_skipped();
        }
        self.first_non_finalized_round_id = round_id.saturating_add(1);
        let value = if let Some(block) = proposal.maybe_block() {
            block.clone()
        } else {
            return outcomes; // This era's last block is already finalized.
        };
        let proposer = self
            .validators
            .id(self.leader(round_id))
            .expect("validator not found")
            .clone();
        let reward = self.rewards.entry(proposer.clone()).or_default();
        *reward = reward.saturating_add(BLOCK_REWARD);
        let terminal_block_data = self.accepted_switch_block(round_id).then(|| {
            let inactive_validators = proposal.inactive().map_or_else(Vec::new, |inactive| {
                inactive
                    .iter()
                    .filter_map(|idx| self.validators.id(*idx))
                    .cloned()
                    .collect()
            });
            TerminalBlockData {
                rewards: self.rewards.clone(),
                inactive_validators,
            }
        });
        let finalized_block = FinalizedBlock {
            value,
            timestamp: proposal.timestamp(),
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

    /// Makes a new proposal if we are the current round leader.
    fn propose_if_leader(
        &mut self,
        maybe_parent_round_id: Option<RoundId>,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        match &self.active_validator {
            Some(active_validator) if active_validator.idx == self.leader(self.current_round) => {}
            _ => return vec![], // Not the current round leader.
        }
        match self.pending_proposal {
            // We already requested a block to propose.
            Some((_, round_id, _)) if round_id == self.current_round => return vec![],
            _ => {}
        }
        if self.round_mut(self.current_round).has_proposal() {
            return vec![]; // We already made a proposal.
        }
        let ancestor_values = match maybe_parent_round_id {
            Some(parent_round_id)
                if self.accepted_switch_block(parent_round_id)
                    || self.accepted_dummy_proposal(parent_round_id) =>
            {
                // One of the ancestors is the switch block, so this proposal has no block.
                return self.create_echo_and_proposal(Proposal::dummy(now, parent_round_id));
            }
            Some(parent_round_id) => self
                .ancestor_values(parent_round_id)
                .expect("missing ancestor value"),
            None => vec![],
        };
        // Request a block payload to propose.
        let block_context = BlockContext::new(now, ancestor_values);
        self.pending_proposal = Some((
            block_context.clone(),
            self.current_round,
            maybe_parent_round_id,
        ));
        vec![ProtocolOutcome::CreateNewBlock(block_context)]
    }

    /// Creates a new proposal message in the current round, and a corresponding signed echo,
    /// inserts them into our protocol state and gossips them.
    fn create_echo_and_proposal(&mut self, proposal: Proposal<C>) -> ProtocolOutcomes<C> {
        let round_id = self.current_round;
        let prop_msg = Message::Proposal {
            round_id,
            proposal: proposal.clone(),
            instance_id: *self.instance_id(),
        };
        let hashed_prop = HashedProposal::new(proposal);
        let mut outcomes = self.create_message(round_id, Content::Echo(*hashed_prop.hash()));
        if outcomes.is_empty() {
            return vec![]; // Failed to create an echo message.
        }
        if !self.record_entry(&Entry::Proposal(hashed_prop.inner().clone(), round_id)) {
            error!("could not record own proposal in WAL");
        } else if self.round_mut(round_id).insert_proposal(hashed_prop) {
            outcomes.push(ProtocolOutcome::CreatedGossipMessage(prop_msg.into()));
        }
        self.mark_dirty(round_id);
        outcomes
    }

    /// Returns a parent if a block with that parent could be proposed in the current round, and the
    /// earliest possible timestamp for a new proposal.
    fn suitable_parent_round(&self, now: Timestamp) -> Option<(Option<RoundId>, Timestamp)> {
        let min_block_time = self.params.min_block_time();
        let mut maybe_parent = None;
        // We iterate through the rounds before the current one, in reverse order.
        for round_id in (0..self.current_round).rev() {
            if let Some((_, parent)) = self.accepted_proposal(round_id) {
                // All rounds higher than this one are skippable. When the accepted proposal's
                // timestamp is old enough it can be used as a parent.
                let timestamp = parent.timestamp().saturating_add(min_block_time);
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
        self.rounds.get(&round_id).and_then(Round::quorum_votes) == Some(false)
    }

    /// Returns whether a quorum has voted for `true`.
    fn is_committed_round(&self, round_id: RoundId) -> bool {
        self.rounds.get(&round_id).and_then(Round::quorum_votes) == Some(true)
    }

    /// Returns whether a round has an accepted proposal.
    fn has_accepted_proposal(&self, round_id: RoundId) -> bool {
        self.round(round_id)
            .and_then(Round::accepted_proposal)
            .is_some()
    }

    /// Returns the accepted proposal, if any, together with its height.
    fn accepted_proposal(&self, round_id: RoundId) -> Option<(u64, &HashedProposal<C>)> {
        self.round(round_id)?.accepted_proposal()
    }

    /// Returns the current proposal timeout as a `TimeDiff`.
    fn proposal_timeout(&self) -> TimeDiff {
        TimeDiff::from_millis(self.proposal_timeout_millis as u64)
    }

    /// Updates our `proposal_timeout` based on the latest measured actual delay from the start of
    /// the current round until a proposal was accepted or we voted to skip the round.
    fn update_proposal_timeout(&mut self, now: Timestamp) {
        let proposal_delay_millis = now.saturating_diff(self.current_round_start).millis() as f64;
        let grace_period_factor = self.config.proposal_grace_period as f64 / 100.0 + 1.0;
        let target_timeout = proposal_delay_millis * grace_period_factor;
        let inertia = self.config.proposal_timeout_inertia as f64;
        let ftt = self.params.ftt().0 as f64 / self.validators.total_weight().0 as f64;
        if target_timeout > self.proposal_timeout_millis {
            self.proposal_timeout_millis *= (1.0 / (inertia * (1.0 - ftt))).exp2();
            self.proposal_timeout_millis = self.proposal_timeout_millis.min(target_timeout);
        } else {
            self.proposal_timeout_millis *= (-1.0 / (inertia * (1.0 + ftt))).exp2();
            let min_timeout = (self.config.proposal_timeout.millis() as f64).max(target_timeout);
            self.proposal_timeout_millis = self.proposal_timeout_millis.max(min_timeout);
        }
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
                sum += self.validators.weight(vidx);
                if sum > quorum_threshold {
                    return true;
                }
            }
        }
        false
    }

    /// Returns the accepted value from the given round and all its ancestors, or `None` if there is
    /// no accepted value in any of those rounds.
    fn ancestor_values(&self, mut round_id: RoundId) -> Option<Vec<C::ConsensusValue>> {
        let mut ancestor_values = vec![];
        loop {
            let (_, proposal) = self.accepted_proposal(round_id)?;
            ancestor_values.extend(proposal.maybe_block().cloned());
            match proposal.maybe_parent_round_id() {
                None => return Some(ancestor_values),
                Some(parent_round_id) => round_id = parent_round_id,
            }
        }
    }

    /// Returns the greatest weight such that two sets of validators with this weight can
    /// intersect in only faulty validators, i.e. have an intersection of weight `<= ftt`. That is
    /// `(total_weight + ftt) / 2`, rounded down. A _quorum_ is any set with a weight strictly
    /// greater than this, so any two quorums have at least one correct validator in common.
    fn quorum_threshold(&self) -> Weight {
        let total_weight = self.validators.total_weight().0;
        let ftt = self.params.ftt().0;
        // sum_overflow is the 33rd bit of the addition's actual result, representing 2^32.
        let (sum, sum_overflow) = total_weight.overflowing_add(ftt);
        if sum_overflow {
            Weight((sum / 2) | 1u64.reverse_bits()) // Add 2^31.
        } else {
            Weight(sum / 2)
        }
    }

    /// Returns the total weight of validators known to be faulty.
    fn faulty_weight(&self) -> Weight {
        self.sum_weights(self.faults.keys())
    }

    /// Returns the sum of the weights of the given validators.
    fn sum_weights<'a>(&self, vidxs: impl Iterator<Item = &'a ValidatorIndex>) -> Weight {
        vidxs.map(|vidx| self.validators.weight(*vidx)).sum()
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
            btree_map::Entry::Vacant(entry) => {
                let leader_idx = self.leader_sequence.leader(u64::from(round_id));
                entry.insert(Round::new(self.validators.len(), leader_idx))
            }
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

impl<C> ConsensusProtocol<C> for Zug<C>
where
    C: Context + 'static,
{
    fn handle_message(
        &mut self,
        _rng: &mut NodeRng,
        sender: NodeId,
        msg: EraMessage<C>,
        now: Timestamp,
    ) -> ProtocolOutcomes<C> {
        match msg.try_into_zug() {
            Err(_msg) => {
                warn!(%sender, "received a message for the wrong consensus protocol");
                vec![ProtocolOutcome::Disconnect(sender)]
            }
            Ok(zug_msg) if zug_msg.instance_id() != self.instance_id() => {
                let instance_id = zug_msg.instance_id();
                warn!(?instance_id, %sender, "wrong instance ID; disconnecting");
                vec![ProtocolOutcome::Disconnect(sender)]
            }
            Ok(Message::SyncResponse(sync_response)) => {
                self.handle_sync_response(sync_response, sender, now)
            }
            Ok(Message::Proposal {
                round_id,
                instance_id: _,
                proposal,
            }) => self.handle_proposal(round_id, proposal, sender, now),
            Ok(Message::Signed(signed_msg)) => self.handle_signed_message(signed_msg, sender, now),
            Ok(Message::Evidence(signed_msg, content2, signature2)) => {
                self.handle_evidence(signed_msg, content2, signature2, sender, now)
            }
        }
    }

    /// Handles an incoming request message and returns an optional response.
    fn handle_request_message(
        &mut self,
        _rng: &mut NodeRng,
        sender: NodeId,
        msg: EraRequest<C>,
        _now: Timestamp,
    ) -> (ProtocolOutcomes<C>, Option<EraMessage<C>>) {
        match msg.try_into_zug() {
            Err(_msg) => {
                warn!(
                    %sender,
                    "received a request for the wrong consensus protocol"
                );
                (vec![ProtocolOutcome::Disconnect(sender)], None)
            }
            Ok(sync_request) if sync_request.instance_id != *self.instance_id() => {
                let instance_id = sync_request.instance_id;
                warn!(?instance_id, %sender, "wrong instance ID; disconnecting");
                (vec![ProtocolOutcome::Disconnect(sender)], None)
            }
            Ok(sync_request) => self.handle_sync_request(sync_request, sender),
        }
    }

    /// Handles the firing of various timers in the protocol.
    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
        now: Timestamp,
        timer_id: TimerId,
        rng: &mut NodeRng,
    ) -> ProtocolOutcomes<C> {
        match timer_id {
            TIMER_ID_SYNC_PEER => self.handle_sync_peer_timer(now, rng),
            TIMER_ID_UPDATE => {
                if timestamp >= self.next_scheduled_update {
                    self.next_scheduled_update = Timestamp::MAX;
                }
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
            // TIMER_ID_VERTEX_WITH_FUTURE_TIMESTAMP => {
            //     self.synchronizer.add_past_due_stored_vertices(now)
            // }
            timer_id => {
                error!(timer_id = timer_id.0, "unexpected timer ID");
                vec![]
            }
        }
    }

    fn handle_is_current(&self, now: Timestamp) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        if let Some(interval) = self.config.sync_state_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()) + interval,
                TIMER_ID_SYNC_PEER,
            ));
        }
        if let Some(interval) = self.config.log_participation_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()) + interval,
                TIMER_ID_LOG_PARTICIPATION,
            ));
        }
        outcomes
    }

    fn handle_action(&mut self, action_id: ActionId, now: Timestamp) -> ProtocolOutcomes<C> {
        error!(?action_id, %now, "unexpected action");
        vec![]
    }

    fn propose(&mut self, proposed_block: ProposedBlock<C>, now: Timestamp) -> ProtocolOutcomes<C> {
        let maybe_parent_round_id = if let Some((block_context, round_id, maybe_parent_round_id)) =
            self.pending_proposal.take()
        {
            if block_context != *proposed_block.context() || round_id != self.current_round {
                warn!(%proposed_block, "skipping outdated proposal");
                self.pending_proposal = Some((block_context, round_id, maybe_parent_round_id));
                return vec![];
            }
            maybe_parent_round_id
        } else {
            error!("unexpected call to propose");
            return vec![];
        };
        let inactive = self
            .validators
            .enumerate_ids()
            .map(|(idx, _)| idx)
            .filter(|idx| self.active[*idx].is_none() && !self.faults.contains_key(idx));
        let proposal = Proposal::with_block(&proposed_block, maybe_parent_round_id, inactive);
        let mut outcomes = self.create_echo_and_proposal(proposal);
        outcomes.extend(self.update(now));
        outcomes
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
        let mut outcomes = vec![];
        if valid {
            for (round_id, proposal, _sender) in rounds_and_node_ids {
                info!(%round_id, %proposal, "handling valid proposal");
                if self.round_mut(round_id).insert_proposal(proposal.clone()) {
                    self.record_entry(&Entry::Proposal(proposal.into_inner(), round_id));
                    self.mark_dirty(round_id);
                    self.progress_detected = true;
                    outcomes.push(ProtocolOutcome::HandledProposedBlock(
                        proposed_block.clone(),
                    ));
                }
            }
            outcomes.extend(self.update(now));
        } else {
            for (round_id, proposal, sender) in rounds_and_node_ids {
                // We don't disconnect from the faulty sender here: The block validator considers
                // the value "invalid" even if it just couldn't download the deploys, which could
                // just be because the original sender went offline.
                let validator_index = self.leader(round_id).0;
                info!(%validator_index, %round_id, %sender, %proposal, "dropping invalid proposal");
            }
        }
        outcomes
    }

    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        now: Timestamp,
        wal_file: Option<PathBuf>,
    ) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        if self.write_wal.is_none() {
            if let Some(wal_file) = wal_file {
                outcomes.extend(self.open_wal(wal_file, now));
            }
            if self.write_wal.is_none() {
                error!(?our_id, "missing WAL file; not activating");
                return vec![];
            }
        }
        if let Some(idx) = self.validators.get_index(&our_id) {
            if self.faults.contains_key(&idx) {
                error!(our_idx = idx.0, "we are faulty; not activating");
                return outcomes;
            }
            info!(our_idx = idx.0, "start voting");
            self.active_validator = Some(ActiveValidator { idx, secret });
            outcomes.extend(self.schedule_update(self.params.start_timestamp().max(now)));
        } else {
            error!(
                ?our_id,
                "we are not a validator in this era; not activating"
            );
        }
        outcomes
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
            let payload = self.create_sync_request(v_idx, round_id).into();
            vec![ProtocolOutcome::CreatedTargetedRequest(payload, peer)]
        } else {
            error!(?vid, "unknown validator ID");
            vec![]
        }
    }

    fn set_paused(&mut self, paused: bool, now: Timestamp) -> ProtocolOutcomes<C> {
        if self.paused && !paused {
            info!(current_round = self.current_round, "unpausing consensus");
            self.paused = paused;
            // Reset the timeout to give the proposer another chance, after the pause.
            self.current_round_start = Timestamp::MAX;
            self.mark_dirty(self.current_round);
            self.update(now)
        } else {
            if self.paused != paused {
                info!(current_round = self.current_round, "pausing consensus");
            }
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
        self.params.instance_id()
    }

    fn next_round_length(&self) -> Option<TimeDiff> {
        Some(self.params.min_block_time())
    }
}
