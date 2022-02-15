use std::{
    any::Any,
    collections::{btree_map, BTreeMap, HashMap, HashSet},
    fmt::Debug,
    path::PathBuf,
};

use datasize::DataSize;
use itertools::Itertools;
use num_traits::AsPrimitive;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use casper_types::{system::auction::BLOCK_REWARD, TimeDiff, Timestamp, U512};

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
        traits::{ConsensusValueT, Context, ValidatorSecret},
        ActionId, TimerId,
    },
    types::{Chainspec, NodeId},
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
            return false;
        }
        // TODO: Detect double-signing!
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
            Content::Vote(vote) => self.votes[&vote][validator_idx].is_some(),
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

    // TODO: Split out protocol state.
    /// Incoming blocks we can't add yet because we are waiting for validation.
    rounds: BTreeMap<RoundId, Round<C>>,
    evidence: ValidatorMap<Option<()>>, // TODO: Define evidence.
    ftt: Weight,
    config: super::highway::config::Config,
    /// The validator's voting weights.
    weights: ValidatorMap<Weight>,
    /// The lowest round ID of a block that could still be finalized in the future.
    first_non_finalized_round_id: RoundId,
    /// The timeout for the current round.
    current_timeout: Timestamp,

    // TODO: Split out leader sequence.
    /// Cumulative validator weights: Entry `i` contains the sum of the weights of validators `0`
    /// through `i`.
    cumulative_w: ValidatorMap<Weight>,
    /// Cumulative validator weights, but with the weight of banned validators set to `0`.
    cumulative_w_leaders: ValidatorMap<Weight>,
    /// A map with `true` for all validators that should be assign slots as leaders, and are
    /// allowed to propose blocks.
    can_propose: ValidatorMap<bool>,
}

impl<C: Context + 'static> SimpleConsensus<C> {
    /// Creates a new boxed `SimpleConsensus` instance.
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
        let ftt_fraction = chainspec.highway_config.finality_threshold_fraction;
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
            .unwrap_or_else(|| chainspec.highway_config.min_round_length());

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
        let mut evidence: ValidatorMap<Option<()>> = validators.iter().map(|_| None).collect();
        for vidx in validators.iter_banned_idx() {
            evidence[vidx] = Some(());
        }

        // For leader selection, we need the list of cumulative weights of the first n validators.
        let sums = |mut sums: Vec<Weight>, w: Weight| {
            let sum = sums.last().copied().unwrap_or(Weight(0));
            // This can't panic: We already scaled down the weights so they're  < 2^64.
            sums.push(sum.checked_add(w).expect("total weight must be < 2^64"));
            sums
        };
        let cumulative_w = ValidatorMap::from(weights.iter().copied().fold(vec![], sums));
        let cumulative_w_leaders = weights
            .enumerate()
            .map(|(idx, weight)| can_propose[idx].then(|| *weight).unwrap_or(Weight(0)))
            .fold(vec![], sums)
            .into();

        info!(
            %proposal_timeout,
            "initializing SimpleConsensus instance",
        );

        // TODO: SimpleConsensus Params
        let params = Params::new(
            seed,
            BLOCK_REWARD,
            (chainspec.highway_config.reduced_reward_multiplier * BLOCK_REWARD).to_integer(),
            chainspec.highway_config.minimum_round_exponent,
            chainspec.highway_config.maximum_round_exponent,
            chainspec.highway_config.minimum_round_exponent,
            chainspec.core_config.minimum_era_height,
            era_start_time,
            era_start_time + chainspec.core_config.era_duration,
            0,
        );

        let sc = Box::new(SimpleConsensus {
            proposals_waiting_for_parent: HashMap::new(),
            proposals_waiting_for_validation: HashMap::new(),
            rounds: BTreeMap::new(),
            first_non_finalized_round_id: 0,
            current_timeout: Timestamp::from(u64::MAX),
            evidence_only: false,
            evidence,
            config: config.highway.clone(),
            params,
            instance_id,
            proposal_timeout,
            validators,
            ftt,
            active_validator: None,
            can_propose,
            weights,
            cumulative_w,
            cumulative_w_leaders,
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
        // TODO: Deduplicate
        let round_id = if let Some(round_id) = self.first_non_finalized_round_id.checked_sub(1) {
            round_id
        } else {
            return false;
        };
        let proposal = self.rounds[&round_id]
            .accepted_proposal
            .as_ref()
            .expect("missing finalized proposal");
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

    /// Returns the leader in the specified time slot.
    ///
    /// First the assignment is computed ignoring the `can_propose` flags. Only if the selected
    /// leader's entry is `false`, the computation is repeated, this time with the flagged
    /// validators excluded. This ensures that once the validator set has been decided, correct
    /// validators' slots never get reassigned to someone else, even if after the fact someone is
    /// excluded as a leader.
    pub(crate) fn leader(&self, round_id: RoundId) -> ValidatorIndex {
        // The binary search cannot return None; if it does, it's a programming error. In that case,
        // we want the tests to panic but production to pick a default.
        let panic_or_0 = || {
            if cfg!(test) {
                panic!("random number out of range");
            } else {
                error!("random number out of range");
                ValidatorIndex(0)
            }
        };
        let seed = self.params.seed().wrapping_add(round_id as u64);
        // We select a random one out of the `total_weight` weight units, starting numbering at 1.
        let r = Weight(leader_prng(self.validators.total_weight().0, seed));
        // The weight units are subdivided into intervals that belong to some validator.
        // `cumulative_w[i]` denotes the last weight unit that belongs to validator `i`.
        // `binary_search` returns the first `i` with `cumulative_w[i] >= r`, i.e. the validator
        // who owns the randomly selected weight unit.
        let leader_index = self
            .cumulative_w
            .binary_search(&r)
            .unwrap_or_else(panic_or_0);
        if self.can_propose[leader_index] {
            return leader_index;
        }
        // If the selected leader is excluded, we reassign the slot to someone else. This time we
        // consider only the non-banned validators.
        let total_w_leaders = *self.cumulative_w_leaders.as_ref().last().unwrap();
        let r = Weight(leader_prng(total_w_leaders.0, seed.wrapping_add(1)));
        self.cumulative_w_leaders
            .binary_search(&r)
            .unwrap_or_else(panic_or_0)
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
                    outcomes.extend(self.check_proposal(round_id));
                    if let Some((our_idx, secret_key)) = &self.active_validator {
                        if self.rounds[&round_id]
                            .echos
                            .values()
                            .all(|echo_map| !echo_map.contains_key(our_idx))
                        {
                            outcomes.extend(self.create_message(round_id, Content::Echo(hash)));
                        }
                    }
                }
            }
            Content::Echo(hash) => {
                if self.add_echo(round_id, validator_idx, hash, signature) {
                    outcomes.extend(self.check_proposal(round_id));
                }
            }
            Content::Vote(vote) => {
                if self
                    .round_mut(round_id)
                    .insert_vote(vote, validator_idx, signature)
                    && self.rounds[&round_id].quorum_votes == None
                    && self.is_quorum(self.rounds[&round_id].votes[&vote].keys_some())
                {
                    self.round_mut(round_id).quorum_votes = Some(vote);
                    if vote {
                        if let Some(proposal) = &self.rounds[&round_id].accepted_proposal {
                            outcomes.extend(self.finalize_round(round_id));
                        }
                    } else {
                        if self.rounds[&round_id].accepted_proposal.is_none()
                            && self.current_round() > round_id
                        {
                            // We started a new round!
                            outcomes.push(ProtocolOutcome::ScheduleTimer(
                                Timestamp::now(),
                                TIMER_ID_ROUND,
                            ));
                        }
                        // This round is now skippable. Check whether proposal in a later round is
                        // now accepted.
                        let rounds_to_check: Vec<RoundId> = self
                            .rounds
                            .range(round_id.saturating_add(1)..)
                            .filter_map(|(future_round_id, round)| {
                                (round.accepted_proposal.is_none() && !round.proposals.is_empty())
                                    .then(|| *future_round_id)
                            })
                            .collect();
                        for future_round_id in rounds_to_check {
                            outcomes.extend(self.check_proposal(future_round_id));
                        }
                    }
                }
            }
        }
        outcomes
    }

    fn check_proposal(&mut self, round_id: RoundId) -> ProtocolOutcomes<C> {
        if self.rounds[&round_id].accepted_proposal.is_some() {
            return vec![]; // We already have an accepted proposal.
        }
        let hash = if let Some(hash) = self.rounds[&round_id].quorum_echos {
            hash
        } else {
            return vec![]; // This round has no quorum of Echos yet.
        };
        let proposal = if let Some((proposal, _)) = self.rounds[&round_id].proposals.get(&hash) {
            proposal.clone()
        } else {
            return vec![];
        };
        let first_skipped_round_id;
        if let Some(parent_round_id) = proposal.maybe_parent_round_id {
            if self
                .rounds
                .get(&parent_round_id)
                .map_or(true, |round| round.accepted_proposal.is_none())
            {
                return vec![]; // Parent is not accepted yet.
            }
            first_skipped_round_id = parent_round_id.saturating_add(1);
        } else {
            first_skipped_round_id = 0;
        }
        if (first_skipped_round_id..round_id)
            .any(|skipped_round_id| !self.is_skippable_round(skipped_round_id))
        {
            return vec![]; // A skipped round is not skippable yet.
        }
        self.round_mut(round_id).accepted_proposal = Some(proposal.clone());
        let mut outcomes = vec![];
        match self.rounds[&round_id].quorum_votes {
            Some(true) => {
                outcomes.extend(self.finalize_round(round_id));
                // We started a new round!
                outcomes.push(ProtocolOutcome::ScheduleTimer(
                    Timestamp::now(),
                    TIMER_ID_ROUND,
                ));
            }
            Some(false) => {} // This round was already skippable.
            None => {
                // We started a new round!
                outcomes.push(ProtocolOutcome::ScheduleTimer(
                    Timestamp::now(),
                    TIMER_ID_ROUND,
                ));
            }
        }
        if let Some((our_idx, secret_key)) = &self.active_validator {
            if self.rounds[&round_id].votes[&true][*our_idx].is_none()
                && self.rounds[&round_id].votes[&false][*our_idx].is_none()
            {
                outcomes.extend(self.create_message(round_id, Content::Vote(true)));
            }
        }
        if let Some(proposals) = self.proposals_waiting_for_parent.remove(&round_id) {
            let ancestor_values = self
                .ancestor_values(round_id)
                .expect("missing ancestors of accepted proposal");
            for (proposal, rounds_and_senders) in proposals {
                for (proposal_round_id, sender, signature) in rounds_and_senders {
                    let validator_idx = self.leader(proposal_round_id);
                    // TODO: Duplicated in handle_message.
                    if proposal.block.needs_validation() {
                        self.log_proposal(
                            &proposal,
                            validator_idx,
                            "requesting proposal validation",
                        );
                        let block_context =
                            BlockContext::new(proposal.timestamp, ancestor_values.clone());
                        let proposed_block =
                            ProposedBlock::new(proposal.block.clone(), block_context);
                        if self
                            .proposals_waiting_for_validation
                            .entry(proposed_block.clone())
                            .or_default()
                            .insert((proposal_round_id, Some(round_id), sender, signature))
                        {
                            outcomes.push(ProtocolOutcome::ValidateConsensusValue {
                                sender,
                                proposed_block,
                            });
                        }
                    } else {
                        self.log_proposal(
                            &proposal,
                            validator_idx,
                            "proposal does not need validation",
                        );
                        outcomes.extend(self.handle_content(
                            proposal_round_id,
                            Content::Proposal(proposal.clone()),
                            validator_idx,
                            signature,
                        ));
                    }
                }
            }
        }
        outcomes
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
                .map(|v| (v.id().clone(), v.weight().0.into()))
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

    /// Adds an `Echo` and returns whether we have a newly reached quorum of Echos.
    fn add_echo(
        &mut self,
        round_id: RoundId,
        validator_idx: ValidatorIndex,
        hash: C::Hash,
        signature: C::Signature,
    ) -> bool {
        if self
            .round_mut(round_id)
            .insert_echo(hash, validator_idx, signature)
            && self.rounds[&round_id].quorum_echos.is_none()
            && self.is_quorum(self.rounds[&round_id].echos[&hash].keys().copied())
        {
            self.round_mut(round_id).quorum_echos = Some(hash);
            true
        } else {
            false
        }
    }

    /// Returns `true` if the given validators, together will all faulty validators, form a quorum.
    fn is_quorum(&self, vidxs: impl Iterator<Item = ValidatorIndex>) -> bool {
        let mut sum: Weight = self
            .evidence
            .iter_some()
            .map(|(vidx, _)| self.weights[vidx])
            .sum();
        let quorum = self.quorum();
        if sum >= quorum {
            return true;
        }
        for vidx in vidxs {
            if self.evidence[vidx].is_none() {
                sum += self.weights[vidx];
                if sum >= quorum {
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

    /// Returns the smallest weight such that any two sets of validators with this weight overlap in
    /// at least one honest validator, i.e. in more than ftt validators.
    fn quorum(&self) -> Weight {
        let total_weight = self.validators.total_weight().0;
        let ftt = self.ftt.0;
        Weight(total_weight / 2 + ftt / 2 + 1 + (total_weight & ftt & 1))
    }

    fn round_mut(&mut self, round_id: RoundId) -> &mut Round<C> {
        match self.rounds.entry(round_id) {
            btree_map::Entry::Occupied(entry) => entry.into_mut(),
            btree_map::Entry::Vacant(entry) => entry.insert(Round::new(self.weights.len())),
        }
    }
}

/// Returns a pseudorandom `u64` between `1` and `upper` (inclusive).
fn leader_prng(upper: u64, seed: u64) -> u64 {
    ChaCha8Rng::seed_from_u64(seed)
        .gen_range(0..upper)
        .saturating_add(1)
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

        if round_id > self.current_round() + MAX_FUTURE_ROUNDS {
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

        if self.evidence.get(validator_idx) == Some(&Some(())) {
            debug!("message from faulty validator");
            // TODO: Determine when we need to store this.
        }

        let mut outcomes = vec![];

        match &content {
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
                            .entry(proposal.clone())
                            .or_insert_with(HashSet::new)
                            .insert((round_id, sender, signature));
                        return vec![];
                    }
                } else {
                    vec![]
                };

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
                        outcomes.push(ProtocolOutcome::ValidateConsensusValue {
                            sender,
                            proposed_block,
                        });
                    }
                    return outcomes;
                } else {
                    self.log_proposal(
                        &proposal,
                        validator_idx,
                        "proposal does not need validation",
                    );
                }
            }
            Content::Echo(_) | Content::Vote(_) => {}
        }
        outcomes.extend(self.handle_content(round_id, content, validator_idx, signature));
        outcomes
    }

    fn handle_timer(&mut self, now: Timestamp, timer_id: TimerId) -> ProtocolOutcomes<C> {
        match timer_id {
            TIMER_ID_ROUND => {
                self.current_timeout = now + self.proposal_timeout;
                let mut outcomes = vec![ProtocolOutcome::ScheduleTimer(
                    self.current_timeout,
                    TIMER_ID_PROPOSAL_TIMEOUT,
                )];
                let current_round = self.current_round();
                let validator_count = self.weights.len();
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
                if let Some((our_idx, secret_key)) = &self.active_validator {
                    if now >= self.current_timeout
                        && self.rounds[&round_id].votes[&true][*our_idx].is_none()
                        && self.rounds[&round_id].votes[&false][*our_idx].is_none()
                    {
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
            for (round_id, maybe_parent_round_id, sender, signature) in rounds_and_node_ids {
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
            .map_or(false, |idx| self.evidence[idx].is_some())
    }

    fn mark_faulty(&mut self, vid: &C::ValidatorId) {
        if let Some(idx) = self.validators.get_index(vid) {
            if self.evidence[idx].is_none() {
                self.evidence[idx] = Some(());
            }
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
        vec![] // TODO: Return all validators who double-signed in this instance.
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
