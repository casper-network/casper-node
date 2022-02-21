use std::{
    any::Any,
    collections::{btree_map, BTreeMap, HashMap, HashSet},
    fmt::Debug,
    path::PathBuf,
};

use datasize::DataSize;
use itertools::Itertools;
use num_traits::AsPrimitive;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use casper_types::{system::auction::BLOCK_REWARD, U512};

use crate::{
    components::consensus::{
        config::{Config, ProtocolConfig},
        consensus_protocol::{
            BlockContext, ConsensusProtocol, FinalizedBlock, ProposedBlock, ProtocolOutcome,
            ProtocolOutcomes, TerminalBlockData,
        },
        highway_core::{
            state::{weight::Weight, Params},
            validators::{ValidatorIndex, ValidatorMap, Validators},
        },
        traits::{ConsensusValueT, Context, ValidatorSecret},
        ActionId, LeaderSequence, TimerId,
    },
    types::{NodeId, TimeDiff, Timestamp},
    utils::ds,
    NodeRng,
};

/// The timer starting a new round.
const TIMER_ID_ROUND: TimerId = TimerId(0);
/// The timer for syncing with a random peer.
const TIMER_ID_SYNC_PEER: TimerId = TimerId(1);
/// The timer for voting to make a round skippable if no proposal was accepted.
const TIMER_ID_PROPOSAL_TIMEOUT: TimerId = TimerId(2);
/// The timer for logging inactive validators.
const TIMER_ID_LOG_PARTICIPATION: TimerId = TimerId(3);
/// The timer for an alert no progress was made in a long time.
const TIMER_ID_STANDSTILL_ALERT: TimerId = TimerId(4);

/// The maximum number of future rounds we instantiate if we get messages from rounds that we
/// haven't started yet.
const MAX_FUTURE_ROUNDS: u32 = 10;

type RoundId = u32;

#[derive(Debug, DataSize)]
struct Round<C>
where
    C: Context,
{
    #[data_size(with = ds::hashmap_sample)]
    proposals: HashMap<C::Hash, (Proposal<C>, C::Signature)>,
    #[data_size(with = ds::hashmap_sample)]
    echos: HashMap<C::Hash, BTreeMap<ValidatorIndex, C::Signature>>,
    votes: BTreeMap<bool, ValidatorMap<Option<C::Signature>>>,

    accepted_proposal: Option<Proposal<C>>,
    quorum_echos: Option<C::Hash>,
    quorum_votes: Option<bool>,
}

impl<C: Context> Round<C> {
    fn new(validator_count: usize) -> Round<C> {
        let mut votes = BTreeMap::new();
        votes.insert(false, vec![None; validator_count].into());
        votes.insert(true, vec![None; validator_count].into());
        Round {
            proposals: HashMap::new(),
            echos: HashMap::new(),
            votes,
            accepted_proposal: None,
            quorum_echos: None,
            quorum_votes: None,
        }
    }

    /// Inserts a `Proposal` and returns its `hash`. Returns `None` if we already had it.
    fn insert_proposal(
        &mut self,
        proposal: Proposal<C>,
        signature: C::Signature,
    ) -> Option<C::Hash> {
        let hash = proposal.hash();
        self.proposals
            .insert(hash, (proposal, signature))
            .is_none()
            .then(|| hash)
        // TODO: Detect double-signing!
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
        // TODO: Detect double-signing!
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
        // TODO: Detect double-signing!
    }

    /// Returns whether the validator has already sent an `Echo` in this round.
    fn has_echoed(&self, validator_idx: ValidatorIndex) -> bool {
        self.echos
            .values()
            .any(|echo_map| echo_map.contains_key(&validator_idx))
    }

    /// Returns whether the validator has already cast a `true` or `false` vote.
    fn has_voted(&self, validator_idx: ValidatorIndex) -> bool {
        self.votes[&true][validator_idx].is_some() && self.votes[&false][validator_idx].is_some()
    }
}

impl<C: Context> Round<C> {
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

#[derive(DataSize, Debug)]
pub(crate) struct SimpleConsensus<C>
where
    C: Context,
{
    params: Params,
    instance_id: C::InstanceId,
    proposal_timeout: TimeDiff,
    validators: Validators<C::ValidatorId>,
    active_validator: Option<(ValidatorIndex, C::ValidatorSecret)>,
    evidence_only: bool,
    proposals_waiting_for_parent:
        HashMap<RoundId, HashMap<Proposal<C>, HashSet<(RoundId, NodeId, C::Signature)>>>,
    proposals_waiting_for_validation:
        HashMap<ProposedBlock<C>, HashSet<(RoundId, Option<RoundId>, NodeId, C::Signature)>>,
    /// If we requested a new block from the block proposer component this contains the proposal's
    /// round ID and the parent's round ID, if there is a parent.
    pending_proposal_round_ids: Option<(RoundId, Option<RoundId>)>,
    leader_sequence: LeaderSequence,
    /// Incoming blocks we can't add yet because we are waiting for validation.
    rounds: BTreeMap<RoundId, Round<C>>,
    /// List of faulty validators and their type of fault.
    faults: HashMap<ValidatorIndex, Fault<C>>,
    ftt: Weight,
    config: super::highway::config::Config,
    /// The validator's voting weights.
    weights: ValidatorMap<Weight>,
    /// The lowest round ID of a block that could still be finalized in the future.
    first_non_finalized_round_id: RoundId,
    /// The timeout for the current round.
    current_timeout: Timestamp,
}

impl<C: Context + 'static> SimpleConsensus<C> {
    /// Creates a new boxed `SimpleConsensus` instance.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub(crate) fn new_boxed(
        instance_id: C::InstanceId,
        validator_stakes: BTreeMap<C::ValidatorId, U512>,
        faulty: &HashSet<C::ValidatorId>,
        inactive: &HashSet<C::ValidatorId>,
        protocol_config: &ProtocolConfig,
        config: &Config,
        prev_cp: Option<&dyn ConsensusProtocol<C>>,
        era_start_time: Timestamp,
        seed: u64,
        now: Timestamp,
    ) -> (Box<dyn ConsensusProtocol<C>>, ProtocolOutcomes<C>) {
        // TODO: Duplicated in HighwayProtocol.
        let sum_stakes: U512 = validator_stakes.iter().map(|(_, stake)| *stake).sum();
        assert!(
            !sum_stakes.is_zero(),
            "cannot start era with total weight 0"
        );
        // We use u64 weights. Scale down by  sum / u64::MAX,  rounded up.
        // If we round up the divisor, the resulting sum is guaranteed to be  <= u64::MAX.
        let scaling_factor = (sum_stakes + U512::from(u64::MAX) - 1) / U512::from(u64::MAX);
        let scale_stake = |(key, stake): (C::ValidatorId, U512)| {
            (key, AsPrimitive::<u64>::as_(stake / scaling_factor))
        };
        // TODO: Sort validators by descending weight.
        let mut validators: Validators<C::ValidatorId> =
            validator_stakes.into_iter().map(scale_stake).collect();
        let weights = ValidatorMap::from(validators.iter().map(|v| v.weight()).collect_vec());

        let total_weight = u128::from(validators.total_weight());
        let ftt_fraction = protocol_config.highway.finality_threshold_fraction;
        assert!(
            ftt_fraction < 1.into(),
            "finality threshold must be less than 100%"
        );
        #[allow(clippy::integer_arithmetic)] // FTT is less than 1, so this can't overflow.
        let ftt = total_weight * *ftt_fraction.numer() as u128 / *ftt_fraction.denom() as u128;
        let ftt: Weight = (ftt as u64).into();

        // Use the estimate from the previous era as the proposal timeout. Start with one minimum
        // round length.
        let proposal_timeout = prev_cp
            .and_then(|cp| cp.as_any().downcast_ref::<SimpleConsensus<C>>())
            .map(|sc| sc.proposal_timeout)
            .unwrap_or_else(|| protocol_config.highway.min_round_length());

        // Validators already known as faulty can be ignored. Validators that were faulty or
        // inactive in the previous era are excluded from being proposer.
        for vid in inactive {
            validators.set_cannot_propose(vid);
        }
        for vid in faulty {
            validators.ban(vid); // This automatically exludes them from proposing, too.
        }
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
            %proposal_timeout,
            "initializing SimpleConsensus instance",
        );

        // TODO: SimpleConsensus Params
        let params = Params::new(
            seed,
            BLOCK_REWARD,
            (protocol_config.highway.reduced_reward_multiplier * BLOCK_REWARD).to_integer(),
            protocol_config.highway.minimum_round_exponent,
            protocol_config.highway.maximum_round_exponent,
            protocol_config.highway.minimum_round_exponent,
            protocol_config.minimum_era_height,
            era_start_time,
            era_start_time + protocol_config.era_duration,
            0,
        );

        let sc = Box::new(SimpleConsensus {
            leader_sequence,
            proposals_waiting_for_parent: HashMap::new(),
            proposals_waiting_for_validation: HashMap::new(),
            rounds: BTreeMap::new(),
            first_non_finalized_round_id: 0,
            current_timeout: Timestamp::from(u64::MAX),
            evidence_only: false,
            faults,
            config: config.highway.clone(),
            params,
            instance_id,
            proposal_timeout,
            validators,
            ftt,
            active_validator: None,
            weights,
            pending_proposal_round_ids: None,
        });

        let mut outcomes = vec![];

        // Start the timer to periodically sync the state with a random peer.
        // TODO: In this protocol the interval should be shorter than in Highway.
        if let Some(interval) = sc.config.request_state_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now.max(sc.params.start_timestamp()) + interval,
                TIMER_ID_SYNC_PEER,
            ));
        }

        (sc, outcomes)
    }

    /// Prints a log statement listing the inactive and faulty validators.
    fn log_participation(&self) {
        info!("validator participation log not implemented yet"); // TODO
    }

    /// Returns whether the switch block has already been finalized.
    fn finalized_switch_block(&self) -> bool {
        let round_id = if let Some(round_id) = self.first_non_finalized_round_id.checked_sub(1) {
            round_id
        } else {
            return false;
        };
        let proposal = self.rounds[&round_id]
            .accepted_proposal
            .as_ref()
            .expect("missing finalized proposal");
        // TODO: Don't require computing ancestor values. (Add height to proposal?)
        let relative_height_plus_1 = self
            .ancestor_values(round_id)
            .expect("missing ancestors of finalized block")
            .len() as u64;
        relative_height_plus_1 >= self.params.end_height()
            && proposal.timestamp >= self.params.end_timestamp()
    }

    /// Request the latest state from a random peer.
    fn handle_sync_peer_timer(&mut self, now: Timestamp) -> ProtocolOutcomes<C> {
        if self.evidence_only || self.finalized_switch_block() {
            return vec![]; // Era has ended. No further progress is expected.
        }
        debug!(
            instance_id = ?self.instance_id,
            "syncing with random peer",
        );
        // Inform a peer about our protocol state and schedule the next request.
        let mut outcomes = self.sync_request();
        if let Some(interval) = self.config.request_state_interval {
            outcomes.push(ProtocolOutcome::ScheduleTimer(
                now + interval,
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
        // TODO: Detect lack of progress.
        if false {
            return vec![ProtocolOutcome::StandstillAlert]; // No progress within the timeout.
        }
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

    /// Creates a message to send our protocol state info to a random peer.
    fn sync_request(&self) -> ProtocolOutcomes<C> {
        // TODO
        // let request: Message<C> = Message::SyncState { .. };
        // let payload = (&request).serialize();
        // vec![ProtocolOutcome::CreatedMessageToRandomPeer(payload)]
        vec![]
    }

    /// Returns the leader in the specified round.
    pub(crate) fn leader(&self, round_id: RoundId) -> ValidatorIndex {
        self.leader_sequence.leader(u64::from(round_id))
    }

    /// Returns the first round that is neither skippable nor has an accepted proposal.
    fn current_round(&self) -> RoundId {
        // The round after the latest known accepted proposal:
        let after_last_accepted = self
            .rounds
            .iter()
            .rev()
            .find(|(_, round)| round.accepted_proposal.is_some())
            .map_or(0, |(round_id, _)| round_id.saturating_add(1));
        (after_last_accepted..)
            .find(|round_id| !self.is_skippable_round(*round_id))
            .unwrap_or(RoundId::MAX)
    }

    fn create_message(&mut self, round_id: RoundId, content: Content<C>) -> ProtocolOutcomes<C> {
        let (validator_idx, secret_key) =
            if let Some((validator_idx, secret_key)) = &self.active_validator {
                (*validator_idx, secret_key)
            } else {
                error!("cannot create message; not a validator");
                return vec![];
            };
        let serialized_fields =
            bincode::serialize(&(round_id, &self.instance_id, &content, validator_idx))
                .expect("failed to serialize fields");
        let hash = <C as Context>::hash(&serialized_fields);
        let signature = secret_key.sign(&hash);
        let mut outcomes = self.handle_content(round_id, content.clone(), validator_idx, signature);
        let message = Message {
            round_id,
            instance_id: self.instance_id,
            content,
            validator_idx,
            signature,
        };
        let serialized_message = message.serialize();
        outcomes.push(ProtocolOutcome::CreatedGossipMessage(serialized_message));
        outcomes
    }

    fn handle_content(
        &mut self,
        round_id: RoundId,
        content: Content<C>,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        // TODO: Detect if ftt is exceeded or there are conflicting quorums.
        match content {
            Content::Proposal(proposal) => {
                if let Some(hash) = self
                    .round_mut(round_id)
                    .insert_proposal(proposal, signature)
                {
                    // The proposal is new; send an Echo and check if it's already accepted.
                    outcomes.extend(self.check_proposal(round_id));
                    if let Some((our_idx, _)) = &self.active_validator {
                        if !self.rounds[&round_id].has_echoed(*our_idx) {
                            outcomes.extend(self.create_message(round_id, Content::Echo(hash)));
                        }
                    }
                }
            }
            Content::Echo(hash) => {
                if self
                    .round_mut(round_id)
                    .insert_echo(hash, validator_idx, signature)
                    && self.rounds[&round_id].quorum_echos.is_none()
                    && self.is_quorum(self.rounds[&round_id].echos[&hash].keys().copied())
                {
                    // The new Echo made us cross the quorum threshold.
                    self.round_mut(round_id).quorum_echos = Some(hash);
                    outcomes.extend(self.check_proposal(round_id));
                }
            }
            Content::Vote(vote) => {
                if self
                    .round_mut(round_id)
                    .insert_vote(vote, validator_idx, signature)
                    && self.rounds[&round_id].quorum_votes.is_none()
                    && self.is_quorum(self.rounds[&round_id].votes[&vote].keys_some())
                {
                    // The new Vote made us cross the quorum threshold.
                    self.round_mut(round_id).quorum_votes = Some(vote);
                    if vote {
                        // This round is committed now. If there is already an accepted proposal,
                        // it is finalized.
                        if self.rounds[&round_id].accepted_proposal.is_some() {
                            outcomes.extend(self.finalize_round(round_id));
                        }
                    } else {
                        // This round is skippable now. If there wasn't already an accepted
                        // proposal, this starts the next round.
                        if self.rounds[&round_id].accepted_proposal.is_none()
                            && self.current_round() > round_id
                        {
                            let now = Timestamp::now();
                            outcomes.push(ProtocolOutcome::ScheduleTimer(now, TIMER_ID_ROUND));
                        }
                        // Check whether proposal in a later round is now accepted.
                        for future_round_id in
                            round_id.saturating_add(1)..=*self.rounds.keys().last().unwrap_or(&0)
                        {
                            outcomes.extend(self.check_proposal(future_round_id));
                        }
                    }
                }
            }
        }
        outcomes
    }

    /// Checks whether a proposal in this round has just become accepted.
    /// If that's the case, it sends a `Vote` message (unless already voted), checks and announces
    /// finality, and checks whether this causes future proposals to become accepted.
    fn check_proposal(&mut self, round_id: RoundId) -> ProtocolOutcomes<C> {
        let hash = if let Some(hash) = self.round(round_id).and_then(|round| round.quorum_echos) {
            hash
        } else {
            return vec![]; // This round has no quorum of Echos yet.
        };
        if self.rounds[&round_id].accepted_proposal.is_some() {
            return vec![]; // We already have an accepted proposal.
        }
        let proposal = if let Some((proposal, _)) = self.rounds[&round_id].proposals.get(&hash) {
            proposal.clone()
        } else {
            return vec![]; // We have a quorum of Echos but no proposal yet.
        };
        let first_skipped_round_id = if let Some(parent_round_id) = proposal.maybe_parent_round_id {
            if self
                .round(parent_round_id)
                .map_or(true, |round| round.accepted_proposal.is_none())
            {
                return vec![]; // Parent is not accepted yet.
            }
            parent_round_id.saturating_add(1)
        } else {
            0
        };
        if (first_skipped_round_id..round_id)
            .any(|skipped_round_id| !self.is_skippable_round(skipped_round_id))
        {
            return vec![]; // A skipped round is not skippable yet.
        }

        // We have a proposal with accepted parent, a quorum of Echos, and all rounds since the
        // parent are skippable. That means the proposal is now accepted.
        self.round_mut(round_id).accepted_proposal = Some(proposal);

        let mut outcomes = vec![];

        // Unless the round was already skippable (quorum of Vote(false)), the newly accepted
        // proposal causes the next round to start. If the round was committed (quorum of
        // Vote(true)), the proposal is finalized.
        if self.rounds[&round_id].quorum_votes != Some(false) {
            let now = Timestamp::now();
            outcomes.push(ProtocolOutcome::ScheduleTimer(now, TIMER_ID_ROUND));
            if self.rounds[&round_id].quorum_votes == Some(true) {
                outcomes.extend(self.finalize_round(round_id)); // Proposal is finalized!
            }
        }

        // If we haven't already voted, we vote to commit and finalize the accepted proposal.
        if let Some((our_idx, _)) = &self.active_validator {
            if !self.rounds[&round_id].has_voted(*our_idx) {
                outcomes.extend(self.create_message(round_id, Content::Vote(true)));
            }
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
        outcomes
    }

    /// Sends a proposal to the `BlockValidator` component for validation. If no validation is
    /// needed, immediately calls `handle_content`.
    fn validate_proposal(
        &mut self,
        round_id: RoundId,
        proposal: Proposal<C>,
        ancestor_values: Vec<C::ConsensusValue>,
        sender: NodeId,
        signature: C::Signature,
    ) -> ProtocolOutcomes<C> {
        let validator_idx = self.leader(round_id);
        if proposal.block.needs_validation() {
            self.log_proposal(&proposal, validator_idx, "requesting proposal validation");
            let block_context = BlockContext::new(proposal.timestamp, ancestor_values);
            let proposed_block = ProposedBlock::new(proposal.block.clone(), block_context);
            if self
                .proposals_waiting_for_validation
                .entry(proposed_block.clone())
                .or_default()
                .insert((round_id, proposal.maybe_parent_round_id, sender, signature))
            {
                vec![ProtocolOutcome::ValidateConsensusValue {
                    sender,
                    proposed_block,
                }]
            } else {
                vec![] // Proposal was already known.
            }
        } else {
            self.log_proposal(
                &proposal,
                validator_idx,
                "proposal does not need validation",
            );
            self.handle_content(
                round_id,
                Content::Proposal(proposal),
                validator_idx,
                signature,
            )
        }
    }

    fn finalize_round(&mut self, round_id: RoundId) -> ProtocolOutcomes<C> {
        let mut outcomes = vec![];
        if round_id < self.first_non_finalized_round_id {
            return outcomes; // This round was already finalized.
        }
        let proposal = self.rounds[&round_id]
            .accepted_proposal
            .as_ref()
            .expect("missing finalized proposal")
            .clone();
        let relative_height = if let Some(parent_round_id) = proposal.maybe_parent_round_id {
            // Output the parent first if it isn't already finalized.
            outcomes.extend(self.finalize_round(parent_round_id));
            self.ancestor_values(parent_round_id)
                .expect("missing ancestors of accepted block")
                .len() as u64
        } else {
            0
        };
        if self.finalized_switch_block() {
            return outcomes; // This era's last block is already finalized.
        }
        self.first_non_finalized_round_id = round_id.saturating_add(1);
        let terminal_block_data = (relative_height.saturating_add(1) >= self.params.end_height()
            && proposal.timestamp >= self.params.end_timestamp())
        .then(|| TerminalBlockData {
            rewards: self
                .validators
                .iter()
                .map(|v| (v.id().clone(), v.weight().0))
                .collect(), // TODO
            inactive_validators: Default::default(), // TODO
        });
        let finalized_block = FinalizedBlock {
            value: proposal.block.clone(),
            timestamp: proposal.timestamp,
            relative_height,
            equivocators: vec![], // TODO
            terminal_block_data,
            proposer: self
                .validators
                .id(self.leader(round_id))
                .expect("validator not found")
                .clone(),
        };
        outcomes.push(ProtocolOutcome::FinalizedBlock(finalized_block));
        outcomes
    }

    fn is_skippable_round(&self, round_id: RoundId) -> bool {
        self.rounds.get(&round_id).map_or(false, |skipped_round| {
            skipped_round.quorum_votes == Some(false)
        })
    }

    /// Returns `true` if the given validators, together will all faulty validators, form a quorum.
    fn is_quorum(&self, vidxs: impl Iterator<Item = ValidatorIndex>) -> bool {
        let mut sum: Weight = self.faults.keys().map(|vidx| self.weights[*vidx]).sum();
        let quorum_threshold = self.quorum_threshold();
        if sum >= quorum_threshold {
            return true;
        }
        for vidx in vidxs {
            if !self.faults.contains_key(&vidx) {
                sum += self.weights[vidx];
                if sum >= quorum_threshold {
                    return true;
                }
            }
        }
        false
    }

    // Returns the accepted value from the given round and all its ancestors, or `None` if there is
    // no accepted value in that round yet.
    fn ancestor_values(&self, mut round_id: RoundId) -> Option<Vec<C::ConsensusValue>> {
        let mut ancestor_values = vec![];
        loop {
            let proposal = self.rounds.get(&round_id)?.accepted_proposal.as_ref()?;
            ancestor_values.push(proposal.block.clone());
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

    fn round(&mut self, round_id: RoundId) -> Option<&Round<C>> {
        self.rounds.get(&round_id)
    }

    fn round_mut(&mut self, round_id: RoundId) -> &mut Round<C> {
        match self.rounds.entry(round_id) {
            btree_map::Entry::Occupied(entry) => entry.into_mut(),
            btree_map::Entry::Vacant(entry) => entry.insert(Round::new(self.weights.len())),
        }
    }
}

#[derive(Clone, Hash, Serialize, Deserialize, Debug, PartialEq, Eq, DataSize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
struct Proposal<C>
where
    C: Context,
{
    timestamp: Timestamp,
    block: C::ConsensusValue,
    maybe_parent_round_id: Option<RoundId>,
}

impl<C: Context> Proposal<C> {
    fn hash(&self) -> C::Hash {
        let serialized = bincode::serialize(&self).expect("failed to serialize fields");
        <C as Context>::hash(&serialized)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
enum Content<C: Context> {
    Proposal(Proposal<C>),
    Echo(C::Hash),
    Vote(bool),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Message<C: Context> {
    // A dependency request. u64 is a random UUID identifying the request.
    // RequestDependency(u64),
    // SyncState { round_id: RoundId },
    round_id: RoundId,
    instance_id: C::InstanceId,
    content: Content<C>,
    validator_idx: ValidatorIndex,
    signature: C::Signature,
}

impl<C: Context> Message<C> {
    pub(crate) fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).expect("should serialize message")
    }
}

impl<C> ConsensusProtocol<C> for SimpleConsensus<C>
where
    C: Context + 'static,
{
    fn handle_message(
        &mut self,
        rng: &mut NodeRng,
        sender: NodeId,
        msg: Vec<u8>,
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
        let Message {
            round_id,
            instance_id: _,
            content,
            validator_idx,
            signature,
        } = match bincode::deserialize::<Message<C>>(msg.as_slice()) {
            Err(err) => {
                let outcome = ProtocolOutcome::InvalidIncomingMessage(msg, sender, err.into());
                return vec![outcome];
            }
            Ok(message) if message.instance_id != self.instance_id => {
                info!(instance_id = ?message.instance_id, ?sender, "wrong instance ID; disconnecting");
                return err_msg("invalid instance ID");
            }
            Ok(message) => message,
        };

        let validator_id = if let Some(validator) = self.validators.id(validator_idx) {
            validator
        } else {
            return err_msg("invalid validator index");
        };

        if round_id > self.current_round().saturating_add(MAX_FUTURE_ROUNDS) {
            debug!(%round_id, "dropping message from future round");
            return vec![];
        }

        if self.evidence_only {
            debug!("received an irrelevant message");
            // TODO: Return vec![] if this isn't an evidence message.
        }

        if self
            .rounds
            .get(&round_id)
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
        if !C::verify_signature(&hash, validator_id, &signature) {
            return err_msg("invalid signature");
        }

        if self
            .faults
            .get(&validator_idx)
            .map_or(false, Fault::is_banned)
        {
            debug!("message from banned validator");
            // TODO: Also drop messages from other faulty validators?
        }

        match content {
            Content::Proposal(proposal) => {
                if proposal.timestamp > now + self.config.pending_vertex_timeout {
                    trace!("received a proposal with a timestamp far in the future; dropping");
                    return vec![];
                }
                if proposal.timestamp > now {
                    trace!("received a proposal with a timestamp slightly in the future");
                    // TODO: If it's not from an equivocator and from the future, add to queue
                    // trace!("received a proposal from the future; storing for later");
                    // let timer_id = TIMER_ID_VERTEX_WITH_FUTURE_TIMESTAMP;
                    // vec![ProtocolOutcome::ScheduleTimer(timestamp, timer_id)]
                    // TODO: Send to block validator, if we already know the parent block.
                    // proposal.maybe_parent_round_id.map_or(true, |parent_round_id|
                    // self.rounds.get(&parent_round_id).and_then(|round|
                    // &round.accepted_proposal).is_some()) &&
                    // return vec![];
                }

                if validator_idx != self.leader(round_id) {
                    return err_msg("wrong leader");
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
                        return vec![];
                    }
                } else {
                    vec![]
                };

                self.validate_proposal(round_id, proposal, ancestor_values, sender, signature)
            }
            content @ Content::Echo(_) | content @ Content::Vote(_) => {
                self.handle_content(round_id, content, validator_idx, signature)
            }
        }
    }

    fn handle_timer(&mut self, now: Timestamp, timer_id: TimerId) -> ProtocolOutcomes<C> {
        match timer_id {
            TIMER_ID_ROUND => {
                // TODO: Increase timeout; reset when rounds get committed.
                self.current_timeout = now + self.proposal_timeout;
                let mut outcomes = vec![ProtocolOutcome::ScheduleTimer(
                    self.current_timeout,
                    TIMER_ID_PROPOSAL_TIMEOUT,
                )];
                let current_round = self.current_round();
                if let Some((our_idx, _)) = self.active_validator {
                    if our_idx == self.leader(current_round)
                        && self.pending_proposal_round_ids.is_none()
                        && self.round_mut(current_round).proposals.is_empty()
                    {
                        let maybe_parent_round_id = (0..current_round)
                            .rev()
                            .find(|round_id| self.rounds[round_id].accepted_proposal.is_some());
                        self.pending_proposal_round_ids =
                            Some((current_round, maybe_parent_round_id));
                        let ancestor_values =
                            maybe_parent_round_id.map_or_else(Vec::new, |parent_round_id| {
                                self.ancestor_values(parent_round_id)
                                    .expect("missing ancestor value")
                            });
                        let block_context = BlockContext::new(now, ancestor_values);
                        // TODO: If after switch block, use empty blocks!
                        // TODO: Stop once switch block is finalized!
                        outcomes.push(ProtocolOutcome::CreateNewBlock(block_context));
                    }
                }
                outcomes
            }
            TIMER_ID_SYNC_PEER => self.handle_sync_peer_timer(now),
            TIMER_ID_PROPOSAL_TIMEOUT => {
                let round_id = self.current_round();
                self.round_mut(round_id);
                if let Some((our_idx, _)) = &self.active_validator {
                    if now >= self.current_timeout && !self.rounds[&round_id].has_voted(*our_idx) {
                        return self.create_message(round_id, Content::Vote(false));
                    }
                }
                vec![]
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
        // Request latest protocol state of the current era.
        let mut outcomes = self.sync_request();
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
        let (block, block_context) = proposed_block.destructure();
        if let Some((proposal_round_id, maybe_parent_round_id)) =
            self.pending_proposal_round_ids.take()
        {
            if self
                .rounds
                .get(&proposal_round_id)
                .expect("missing current round")
                .proposals
                .is_empty()
                && self
                    .active_validator
                    .as_ref()
                    .map_or(false, |(our_idx, _)| {
                        *our_idx == self.leader(proposal_round_id)
                    })
            {
                let content = Content::Proposal(Proposal {
                    timestamp: block_context.timestamp(),
                    block,
                    maybe_parent_round_id,
                });
                self.create_message(proposal_round_id, content)
            } else {
                error!("proposal already exists");
                vec![]
            }
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
            let (block, block_context) = proposed_block.destructure();
            let mut outcomes = vec![];
            for (round_id, maybe_parent_round_id, _sender, signature) in rounds_and_node_ids {
                let proposal = Proposal {
                    block: block.clone(),
                    timestamp: block_context.timestamp(),
                    maybe_parent_round_id,
                };
                outcomes.extend(self.handle_content(
                    round_id,
                    Content::Proposal(proposal),
                    self.leader(round_id),
                    signature,
                ));
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
            return vec![ProtocolOutcome::ScheduleTimer(
                now.max(self.params.start_timestamp()),
                TIMER_ID_ROUND,
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
        // TODO: Drop protocol state.
        self.evidence_only = true;
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

    fn request_evidence(&self, sender: NodeId, vid: &C::ValidatorId) -> ProtocolOutcomes<C> {
        if let Some(idx) = self.validators.get_index(vid) {
            // TODO
            // let msg = Message::<C>::RequestDependency(0);
            // vec![ProtocolOutcome::CreatedTargetedMessage(
            //     msg.serialize(),
            //     sender,
            // )]
            vec![]
        } else {
            error!(?vid, "unknown validator ID");
            vec![]
        }
    }

    /// Does nothing: This protocol doesn't create more protocol state if no quorum is online, so no
    /// special pause mode is needed.
    fn set_paused(&mut self, paused: bool) {}

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
