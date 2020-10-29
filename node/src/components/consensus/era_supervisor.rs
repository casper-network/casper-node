//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

use std::{
    collections::HashMap,
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    rc::Rc,
};

use anyhow::Error;
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use datasize::DataSize;
use itertools::Itertools;
use num_traits::AsPrimitive;
use prometheus::Registry;
use rand::Rng;
use tracing::{error, info, trace, warn};

use casper_execution_engine::{
    core::engine_state::era_validators::GetEraValidatorsRequest, shared::motes::Motes,
};
use casper_types::{
    auction::{ValidatorWeights, AUCTION_DELAY, BLOCK_REWARD, DEFAULT_UNBONDING_DELAY},
    ProtocolVersion, U512,
};

use crate::{
    components::{
        chainspec_loader::{Chainspec, HighwayConfig},
        consensus::{
            candidate_block::CandidateBlock,
            cl_context::{ClContext, ClSecret},
            consensus_protocol::{
                BlockContext, ConsensusProtocol, ConsensusProtocolResult, EraEnd,
                FinalizedBlock as CpFinalizedBlock,
            },
            highway_core::{highway::Params, validators::Validators},
            metrics::ConsensusMetrics,
            protocols::highway::HighwayProtocol,
            traits::NodeIdT,
            Config, ConsensusMessage, Event, ReactorEventT,
        },
    },
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash,
    },
    effect::{EffectBuilder, EffectExt, Effects, Responder},
    types::{BlockHash, BlockHeader, CryptoRngCore, FinalizedBlock, ProtoBlock, Timestamp},
    utils::WithDir,
};

pub use self::era::{Era, EraId};

mod era;

/// The unbonding period, in number of eras. After this many eras, a former validator is allowed to
/// withdraw their stake, so their signature can't be trusted anymore.
///
/// A node keeps `2 * BONDED_ERAS` past eras around, because the oldest bonded era could still
/// receive blocks that refer to `BONDED_ERAS` before that.
const BONDED_ERAS: u64 = DEFAULT_UNBONDING_DELAY - AUCTION_DELAY;

#[derive(DataSize)]
pub struct EraSupervisor<I> {
    /// A map of active consensus protocols.
    /// A value is a trait so that we can run different consensus protocol instances per era.
    ///
    /// This map always contains exactly `2 * BONDED_ERAS + 1` entries, with the last one being the
    /// current one.
    active_eras: HashMap<EraId, Era<I>>,
    pub(super) secret_signing_key: Rc<SecretKey>,
    pub(super) public_signing_key: PublicKey,
    current_era: EraId,
    chainspec: Chainspec,
    node_start_time: Timestamp,
    #[data_size(skip)]
    metrics: ConsensusMetrics,
}

impl<I> Debug for EraSupervisor<I> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        let ae: Vec<_> = self.active_eras.keys().collect();
        write!(formatter, "EraSupervisor {{ active_eras: {:?}, .. }}", ae)
    }
}

impl<I> EraSupervisor<I>
where
    I: NodeIdT,
{
    /// Creates a new `EraSupervisor`, starting in era 0.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new<REv: ReactorEventT<I>>(
        timestamp: Timestamp,
        config: WithDir<Config>,
        effect_builder: EffectBuilder<REv>,
        validator_stakes: Vec<(PublicKey, Motes)>,
        chainspec: &Chainspec,
        genesis_state_root_hash: hash::Digest,
        registry: &Registry,
        mut rng: &mut dyn CryptoRngCore,
    ) -> Result<(Self, Effects<Event<I>>), Error> {
        let (root, config) = config.into_parts();
        let secret_signing_key = Rc::new(config.secret_key_path.load(root)?);
        let public_signing_key = PublicKey::from(secret_signing_key.as_ref());
        let metrics = ConsensusMetrics::new(registry)
            .expect("failure to setup and register ConsensusMetrics");

        let mut era_supervisor = Self {
            active_eras: Default::default(),
            secret_signing_key,
            public_signing_key,
            current_era: EraId(0),
            chainspec: chainspec.clone(),
            node_start_time: Timestamp::now(),
            metrics,
        };

        let results = era_supervisor.new_era(
            EraId(0),
            timestamp,
            validator_stakes,
            vec![], // no banned validators in era 0
            0,      // hardcoded seed for era 0
            chainspec.genesis.highway_config.genesis_era_start_timestamp,
            0, // the first block has height 0
            genesis_state_root_hash,
        );
        let effects = era_supervisor
            .handling_wrapper(effect_builder, &mut rng)
            .handle_consensus_results(EraId(0), results);

        Ok((era_supervisor, effects))
    }

    /// Returns a temporary container with this `EraSupervisor`, `EffectBuilder` and random number
    /// generator, for handling events.
    pub(super) fn handling_wrapper<'a, REv: ReactorEventT<I>>(
        &'a mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &'a mut dyn CryptoRngCore,
    ) -> EraSupervisorHandlingWrapper<'a, I, REv> {
        EraSupervisorHandlingWrapper {
            era_supervisor: self,
            effect_builder,
            rng,
        }
    }

    fn highway_config(&self) -> HighwayConfig {
        self.chainspec.genesis.highway_config
    }

    fn booking_block_height(&self, era_id: EraId) -> u64 {
        // The booking block for era N is the last block of era N - AUCTION_DELAY - 1
        // To find it, we get the start height of era N - AUCTION_DELAY and subtract 1
        let after_booking_era_id = EraId(era_id.0.saturating_sub(AUCTION_DELAY));
        self.active_eras
            .get(&after_booking_era_id)
            .expect("should have era after booking block")
            .start_height
            .saturating_sub(1)
    }

    fn key_block_height(&self, _era_id: EraId, start_height: u64) -> u64 {
        // the switch block of the previous era
        // TODO: consider defining the key block as a block further in the past
        start_height.saturating_sub(1)
    }

    fn era_seed(booking_block_hash: BlockHash, key_block_seed: hash::Digest) -> u64 {
        let mut result = [0; hash::Digest::LENGTH];
        let mut hasher = VarBlake2b::new(hash::Digest::LENGTH).expect("should create hasher");

        hasher.update(booking_block_hash);
        hasher.update(key_block_seed);

        hasher.finalize_variable(|slice| {
            result.copy_from_slice(slice);
        });

        u64::from_le_bytes(result[0..std::mem::size_of::<u64>()].try_into().unwrap())
    }

    /// Starts a new era; panics if it already exists.
    #[allow(clippy::too_many_arguments)] // FIXME
    fn new_era(
        &mut self,
        era_id: EraId,
        timestamp: Timestamp,
        validator_stakes: Vec<(PublicKey, Motes)>,
        newly_slashed: Vec<PublicKey>,
        seed: u64,
        start_time: Timestamp,
        start_height: u64,
        state_root_hash: hash::Digest,
    ) -> Vec<ConsensusProtocolResult<I, CandidateBlock, PublicKey>> {
        if self.active_eras.contains_key(&era_id) {
            panic!("{} already exists", era_id);
        }
        self.current_era = era_id;

        let sum_stakes: Motes = validator_stakes.iter().map(|(_, stake)| *stake).sum();
        assert!(
            !sum_stakes.value().is_zero(),
            "cannot start era with total weight 0"
        );

        let init_round_exp = era_id
            .checked_sub(1)
            .and_then(|last_era_id| self.active_eras.get(&last_era_id))
            .and_then(|era| {
                era.consensus
                    .as_any()
                    .downcast_ref::<HighwayProtocol<I, ClContext>>()
            })
            .and_then(|highway_proto| highway_proto.median_round_exp())
            .unwrap_or(self.highway_config().minimum_round_exponent);

        info!(
            ?validator_stakes,
            %start_time,
            %timestamp,
            %start_height,
            era = era_id.0,
            %init_round_exp,
            "starting era",
        );

        // For Highway, we need u64 weights. Scale down by  sum / u64::MAX,  rounded up.
        // If we round up the divisor, the resulting sum is guaranteed to be  <= u64::MAX.
        let scaling_factor = (sum_stakes.value() + U512::from(u64::MAX) - 1) / U512::from(u64::MAX);
        let scale_stake = |(key, stake): (PublicKey, Motes)| {
            (key, AsPrimitive::<u64>::as_(stake.value() / scaling_factor))
        };
        let mut validators: Validators<PublicKey> =
            validator_stakes.into_iter().map(scale_stake).collect();

        let slashed = era_id
            .iter_other_bonded()
            .flat_map(|e_id| &self.active_eras[&e_id].newly_slashed)
            .chain(&newly_slashed)
            .cloned()
            .collect();

        for pub_key in &slashed {
            validators.ban(pub_key);
        }

        let total_weight = u128::from(validators.total_weight());
        let ftt_percent = u128::from(self.highway_config().finality_threshold_percent);
        let ftt = ((total_weight * ftt_percent / 100) as u64).into();

        // TODO: The initial round length should be the observed median of the switch block.
        let params = Params::new(
            seed,
            BLOCK_REWARD,
            BLOCK_REWARD / 5, // TODO: Make reduced block reward configurable?
            self.highway_config().minimum_round_exponent,
            init_round_exp,
            self.highway_config().minimum_era_height,
            start_time,
            start_time + self.highway_config().era_duration,
        );

        // Activate the era if this node was already running when the era began, it is still
        // ongoing based on its minimum duration, and we are one of the validators.
        let our_id = self.public_signing_key;
        let era_rounds_len = params.min_round_len() * params.end_height();
        let min_end_time = start_time + self.highway_config().era_duration.max(era_rounds_len);
        let should_activate = if self.node_start_time >= start_time {
            info!(
                era = era_id.0,
                %self.node_start_time, "not voting; node was not started before the era began",
            );
            false
        } else if min_end_time < timestamp {
            info!(
                era = era_id.0,
                %min_end_time,
                "not voting; era started too long ago",
            );
            false
        } else if !validators.iter().any(|v| *v.id() == our_id) {
            info!(era = era_id.0, %our_id, "not voting; not a validator");
            false
        } else {
            info!(era = era_id.0, "start voting");
            true
        };

        let mut highway = HighwayProtocol::<I, ClContext>::new(
            instance_id(&self.chainspec, state_root_hash, start_height),
            validators,
            params,
            ftt,
        );

        let results = if should_activate {
            let secret = ClSecret::new(Rc::clone(&self.secret_signing_key), our_id);
            highway.activate_validator(our_id, secret, params, timestamp)
        } else {
            Vec::new()
        };

        let era = Era::new(highway, start_height, newly_slashed, slashed);
        let _ = self.active_eras.insert(era_id, era);

        // Remove the era that has become obsolete now. We keep 2 * BONDED_ERAS past eras because
        // the oldest bonded era could still receive blocks that refer to BONDED_ERAS before that.
        if let Some(obsolete_era_id) = era_id.checked_sub(2 * BONDED_ERAS + 1) {
            trace!(era = obsolete_era_id.0, "removing obsolete era");
            self.active_eras.remove(&obsolete_era_id);
        }

        results
    }

    /// Returns the current era.
    fn current_era_mut(&mut self) -> &mut Era<I> {
        self.active_eras
            .get_mut(&self.current_era)
            .expect("current era does not exist")
    }

    /// Returns `true` if the specified era is active and bonded.
    fn is_bonded(&self, era_id: EraId) -> bool {
        era_id.0 + BONDED_ERAS >= self.current_era.0 && era_id <= self.current_era
    }

    /// Inspect the active eras.
    #[cfg(test)]
    pub(crate) fn active_eras(&self) -> &HashMap<EraId, Era<I>> {
        &self.active_eras
    }
}

/// A mutable `EraSupervisor` reference, together with an `EffectBuilder`.
///
/// This is a short-lived convenience type to avoid passing the effect builder through lots of
/// message calls, and making every method individually generic in `REv`. It is only instantiated
/// for the duration of handling a single event.
pub(super) struct EraSupervisorHandlingWrapper<'a, I, REv: 'static> {
    pub(super) era_supervisor: &'a mut EraSupervisor<I>,
    pub(super) effect_builder: EffectBuilder<REv>,
    pub(super) rng: &'a mut dyn CryptoRngCore,
}

impl<'a, I, REv> EraSupervisorHandlingWrapper<'a, I, REv>
where
    I: NodeIdT,
    REv: ReactorEventT<I>,
{
    /// Applies `f` to the consensus protocol of the specified era.
    fn delegate_to_era<F>(&mut self, era_id: EraId, f: F) -> Effects<Event<I>>
    where
        F: FnOnce(
            &mut dyn ConsensusProtocol<I, CandidateBlock, PublicKey>,
            &mut dyn CryptoRngCore,
        ) -> Vec<ConsensusProtocolResult<I, CandidateBlock, PublicKey>>,
    {
        match self.era_supervisor.active_eras.get_mut(&era_id) {
            None => {
                if era_id > self.era_supervisor.current_era {
                    info!(era = era_id.0, "received message for future era");
                } else {
                    info!(era = era_id.0, "received message for obsolete era");
                }
                Effects::new()
            }
            Some(era) => {
                let results = f(&mut *era.consensus, self.rng);
                self.handle_consensus_results(era_id, results)
            }
        }
    }

    pub(super) fn handle_timer(
        &mut self,
        era_id: EraId,
        timestamp: Timestamp,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.handle_timer(timestamp, rng)
        })
    }

    pub(super) fn handle_message(&mut self, sender: I, msg: ConsensusMessage) -> Effects<Event<I>> {
        match msg {
            ConsensusMessage::Protocol { era_id, payload } => {
                // If the era is already unbonded, only accept new evidence, because still-bonded
                // eras could depend on that.
                let evidence_only = !self.era_supervisor.is_bonded(era_id);
                self.delegate_to_era(era_id, move |consensus, rng| {
                    consensus.handle_message(sender, payload, evidence_only, rng)
                })
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => {
                if !self.era_supervisor.is_bonded(era_id) {
                    trace!(era = era_id.0, "not handling message; era too old");
                    return Effects::new();
                }
                era_id
                    .iter_bonded()
                    .flat_map(|e_id| {
                        self.delegate_to_era(e_id, |consensus, _| {
                            consensus.request_evidence(sender.clone(), &pub_key)
                        })
                    })
                    .collect()
            }
        }
    }

    pub(super) fn handle_new_proto_block(
        &mut self,
        era_id: EraId,
        proto_block: ProtoBlock,
        block_context: BlockContext,
    ) -> Effects<Event<I>> {
        if !self.era_supervisor.is_bonded(era_id) {
            warn!(era = era_id.0, "new proto block in outdated era");
            return Effects::new();
        }
        let mut effects = self
            .effect_builder
            .announce_proposed_proto_block(proto_block.clone())
            .ignore();
        let accusations = era_id
            .iter_bonded()
            .flat_map(|e_id| self.era(e_id).consensus.validators_with_evidence())
            .unique()
            .filter(|pub_key| !self.era(era_id).slashed.contains(pub_key))
            .cloned()
            .collect();
        let candidate_block = CandidateBlock::new(proto_block, accusations);
        effects.extend(self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.propose(candidate_block, block_context, rng)
        }));
        effects
    }

    pub(super) fn handle_linear_chain_block(
        &mut self,
        block_header: BlockHeader,
        responder: Responder<Signature>,
    ) -> Effects<Event<I>> {
        // TODO - we should only sign if we're a validator for the given era ID.
        let signature = asymmetric_key::sign(
            block_header.hash().inner(),
            &self.era_supervisor.secret_signing_key,
            &self.era_supervisor.public_signing_key,
            self.rng,
        );
        let mut effects = responder.respond(signature).ignore();
        if block_header.era_id() < self.era_supervisor.current_era {
            trace!(era_id = %block_header.era_id(), "executed block in old era");
            return effects;
        }
        if block_header.switch_block() {
            // if the block is a switch block, we have to get the validators for the new era and
            // create it, before we can say we handled the block
            let new_era_id = block_header.era_id().successor();
            let request = GetEraValidatorsRequest::new(
                (*block_header.state_root_hash()).into(),
                new_era_id.0,
                ProtocolVersion::V1_0_0,
            );
            let key_block_height = self
                .era_supervisor
                .key_block_height(new_era_id, block_header.height() + 1);
            let booking_block_height = self.era_supervisor.booking_block_height(new_era_id);
            let effect = self
                .effect_builder
                .create_new_era(request, booking_block_height, key_block_height)
                .event(
                    move |(validators, booking_block, key_block)| Event::CreateNewEra {
                        block_header: Box::new(block_header),
                        booking_block_hash: booking_block
                            .map_or_else(|| Err(booking_block_height), |block| Ok(*block.hash())),
                        key_block_seed: key_block.map_or_else(
                            || Err(key_block_height),
                            |block| Ok(block.header().accumulated_seed()),
                        ),
                        get_validators_result: validators,
                    },
                );
            effects.extend(effect);
        } else {
            // if it's not a switch block, we can already declare it handled
            effects.extend(
                self.effect_builder
                    .announce_block_handled(block_header)
                    .ignore(),
            );
        }
        effects
    }

    pub(super) fn handle_create_new_era(
        &mut self,
        block_header: BlockHeader,
        booking_block_hash: BlockHash,
        key_block_seed: hash::Digest,
        validator_weights: ValidatorWeights,
    ) -> Effects<Event<I>> {
        let validator_stakes = validator_weights
            .into_iter()
            .filter_map(|(key, stake)| match key.try_into() {
                Ok(key) => Some((key, Motes::new(stake))),
                Err(error) => {
                    warn!(%error, "error converting the bonded key");
                    None
                }
            })
            .collect();
        self.era_supervisor
            .current_era_mut()
            .consensus
            .deactivate_validator();
        let newly_slashed = block_header
            .era_end()
            .expect("switch block must have era_end")
            .equivocators
            .clone();
        let era_id = block_header.era_id().successor();
        info!(era = era_id.0, "era created");
        let seed = EraSupervisor::<I>::era_seed(booking_block_hash, key_block_seed);
        trace!(%seed, "the seed for {}: {}", era_id, seed);
        let results = self.era_supervisor.new_era(
            era_id,
            Timestamp::now(), // TODO: This should be passed in.
            validator_stakes,
            newly_slashed,
            seed,
            block_header.timestamp(),
            block_header.height() + 1,
            *block_header.state_root_hash(),
        );
        let mut effects = self.handle_consensus_results(era_id, results);
        effects.extend(
            self.effect_builder
                .announce_block_handled(block_header)
                .ignore(),
        );
        effects
    }

    pub(super) fn resolve_validity(
        &mut self,
        era_id: EraId,
        _sender: I, // TODO: Disconnect from sender if invalid.
        proto_block: ProtoBlock,
        valid: bool,
    ) -> Effects<Event<I>> {
        self.era_supervisor.metrics.proposed_block();
        let mut effects = Effects::new();
        let candidate_blocks = if let Some(era) = self.era_supervisor.active_eras.get_mut(&era_id) {
            era.resolve_validity(&proto_block, valid)
        } else {
            return effects;
        };
        for candidate_block in candidate_blocks {
            effects.extend(self.delegate_to_era(era_id, |consensus, rng| {
                consensus.resolve_validity(&candidate_block, valid, rng)
            }));
        }
        if valid {
            effects.extend(
                self.effect_builder
                    .announce_proposed_proto_block(proto_block)
                    .ignore(),
            );
        }
        effects
    }

    fn handle_consensus_results<T>(&mut self, era_id: EraId, results: T) -> Effects<Event<I>>
    where
        T: IntoIterator<Item = ConsensusProtocolResult<I, CandidateBlock, PublicKey>>,
    {
        results
            .into_iter()
            .flat_map(|result| self.handle_consensus_result(era_id, result))
            .collect()
    }

    /// Returns `true` if any of the most recent eras has evidence against the validator with key
    /// `pub_key`.
    fn has_evidence(&self, era_id: EraId, pub_key: PublicKey) -> bool {
        era_id
            .iter_bonded()
            .any(|eid| self.era(eid).consensus.has_evidence(&pub_key))
    }

    /// Returns the era with the specified ID. Panics if it does not exist.
    fn era(&self, era_id: EraId) -> &Era<I> {
        &self.era_supervisor.active_eras[&era_id]
    }

    /// Returns the era with the specified ID mutably. Panics if it does not exist.
    fn era_mut(&mut self, era_id: EraId) -> &mut Era<I> {
        self.era_supervisor.active_eras.get_mut(&era_id).unwrap()
    }

    fn handle_consensus_result(
        &mut self,
        era_id: EraId,
        consensus_result: ConsensusProtocolResult<I, CandidateBlock, PublicKey>,
    ) -> Effects<Event<I>> {
        match consensus_result {
            ConsensusProtocolResult::InvalidIncomingMessage(_, sender, error) => {
                // TODO: we will probably want to disconnect from the sender here
                error!(
                    %sender,
                    %error,
                    "invalid incoming message to consensus instance"
                );
                Default::default()
            }
            ConsensusProtocolResult::CreatedGossipMessage(out_msg) => {
                // TODO: we'll want to gossip instead of broadcast here
                self.effect_builder
                    .broadcast_message(era_id.message(out_msg).into())
                    .ignore()
            }
            ConsensusProtocolResult::CreatedTargetedMessage(out_msg, to) => self
                .effect_builder
                .send_message(to, era_id.message(out_msg).into())
                .ignore(),
            ConsensusProtocolResult::ScheduleTimer(timestamp) => {
                let timediff = timestamp.saturating_sub(Timestamp::now());
                self.effect_builder
                    .set_timeout(timediff.into())
                    .event(move |_| Event::Timer { era_id, timestamp })
            }
            ConsensusProtocolResult::CreateNewBlock { block_context } => self
                .effect_builder
                .request_proto_block(block_context, self.rng.gen())
                .event(move |(proto_block, block_context)| Event::NewProtoBlock {
                    era_id,
                    proto_block,
                    block_context,
                }),
            ConsensusProtocolResult::FinalizedBlock(CpFinalizedBlock {
                value,
                timestamp,
                height,
                rewards,
                equivocators,
                proposer,
            }) => {
                self.era_mut(era_id).add_accusations(&equivocators);
                self.era_mut(era_id).add_accusations(value.accusations());
                // If this is the era's last block, it contains rewards. Everyone who is accused in
                // the block or seen as equivocating via the consensus protocol gets slashed.
                let era_end = rewards.map(|rewards| EraEnd {
                    rewards,
                    equivocators: self.era(era_id).accusations(),
                });
                let finalized_block = FinalizedBlock::new(
                    value.proto_block().clone(),
                    timestamp,
                    era_end,
                    era_id,
                    self.era(era_id).start_height + height,
                    proposer,
                );
                self.era_supervisor
                    .metrics
                    .finalized_block(&finalized_block);
                // Announce the finalized proto block.
                let mut effects = self
                    .effect_builder
                    .announce_finalized_block(finalized_block.clone())
                    .ignore();
                // Request execution of the finalized block.
                effects.extend(self.effect_builder.execute_block(finalized_block).ignore());
                effects
            }
            ConsensusProtocolResult::ValidateConsensusValue(sender, candidate_block) => {
                if !self.era_supervisor.is_bonded(era_id) {
                    return Effects::new();
                }
                let proto_block = candidate_block.proto_block().clone();
                let missing_evidence: Vec<PublicKey> = candidate_block
                    .accusations()
                    .iter()
                    .filter(|pub_key| !self.has_evidence(era_id, **pub_key))
                    .cloned()
                    .collect();
                let mut effects = Effects::new();
                for pub_key in missing_evidence.iter().cloned() {
                    let msg = ConsensusMessage::EvidenceRequest { era_id, pub_key };
                    effects.extend(
                        self.effect_builder
                            .send_message(sender.clone(), msg.into())
                            .ignore(),
                    );
                }
                self.era_mut(era_id)
                    .add_candidate(candidate_block, missing_evidence);
                effects.extend(
                    self.effect_builder
                        .validate_block(sender.clone(), proto_block)
                        .event(move |(valid, proto_block)| Event::ResolveValidity {
                            era_id,
                            sender,
                            proto_block,
                            valid,
                        }),
                );
                effects
            }
            ConsensusProtocolResult::NewEvidence(pub_key) => {
                let mut effects = Effects::new();
                for e_id in (era_id.0..=(era_id.0 + BONDED_ERAS)).map(EraId) {
                    let candidate_blocks =
                        if let Some(era) = self.era_supervisor.active_eras.get_mut(&e_id) {
                            era.resolve_evidence(&pub_key)
                        } else {
                            continue;
                        };
                    for candidate_block in candidate_blocks {
                        effects.extend(self.delegate_to_era(e_id, |consensus, rng| {
                            consensus.resolve_validity(&candidate_block, true, rng)
                        }));
                    }
                }
                effects
            }
            ConsensusProtocolResult::SendEvidence(sender, pub_key) => era_id
                .iter_other_bonded()
                .flat_map(|e_id| {
                    self.delegate_to_era(e_id, |consensus, _| {
                        consensus.request_evidence(sender.clone(), &pub_key)
                    })
                })
                .collect(),
        }
    }
}

/// Computes the instance ID for an era, given the state root hash, block height and chainspec.
fn instance_id(
    chainspec: &Chainspec,
    state_root_hash: hash::Digest,
    block_height: u64,
) -> hash::Digest {
    let mut result = [0; hash::Digest::LENGTH];
    let mut hasher = VarBlake2b::new(hash::Digest::LENGTH).expect("should create hasher");

    hasher.update(&chainspec.genesis.name);
    hasher.update(chainspec.genesis.timestamp.millis().to_le_bytes());
    hasher.update(state_root_hash);

    for upgrade_point in chainspec
        .upgrades
        .iter()
        .take_while(|up| up.activation_point.rank <= block_height)
    {
        hasher.update(upgrade_point.activation_point.rank.to_le_bytes());
        if let Some(bytes) = upgrade_point.upgrade_installer_bytes.as_ref() {
            hasher.update(bytes);
        }
        if let Some(bytes) = upgrade_point.upgrade_installer_args.as_ref() {
            hasher.update(bytes);
        }
    }

    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    result.into()
}
