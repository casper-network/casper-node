//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::Error;
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use datasize::DataSize;
use itertools::Itertools;
use prometheus::Registry;
use rand::Rng;
use tracing::{debug, info, trace, warn};

use casper_types::{AsymmetricType, PublicKey, SecretKey, U512};

use crate::{
    components::consensus::{
        candidate_block::CandidateBlock,
        cl_context::{ClContext, Keypair},
        consensus_protocol::{
            BlockContext, ConsensusProtocol, EraEnd, FinalizedBlock as CpFinalizedBlock,
            ProtocolOutcome,
        },
        metrics::ConsensusMetrics,
        traits::NodeIdT,
        ActionId, Config, ConsensusMessage, Event, ReactorEventT, TimerId,
    },
    crypto::hash::Digest,
    effect::{EffectBuilder, EffectExt, Effects, Responder},
    fatal,
    types::{
        ActivationPoint, BlockHash, BlockHeader, BlockLike, FinalitySignature, FinalizedBlock,
        ProtoBlock, Timestamp,
    },
    utils::WithDir,
    NodeRng,
};

pub use self::era::{Era, EraId};
use crate::components::consensus::config::ProtocolConfig;

mod era;

type ConsensusConstructor<I> = dyn Fn(
    Digest,                                       // the era's unique instance ID
    BTreeMap<PublicKey, U512>,                    // validator weights
    &HashSet<PublicKey>,                          // slashed validators that are banned in this era
    &ProtocolConfig,                              // the network's chainspec
    &Config,                                      // The consensus part of the node config.
    Option<&dyn ConsensusProtocol<I, ClContext>>, // previous era's consensus instance
    Timestamp,                                    // start time for this era
    u64,                                          // random seed
) -> (
    Box<dyn ConsensusProtocol<I, ClContext>>,
    Vec<ProtocolOutcome<I, ClContext>>,
) + Send;

#[derive(DataSize)]
pub struct EraSupervisor<I> {
    /// A map of active consensus protocols.
    /// A value is a trait so that we can run different consensus protocol instances per era.
    ///
    /// This map always contains exactly `2 * bonded_eras + 1` entries, with the last one being the
    /// current one.
    active_eras: HashMap<EraId, Era<I>>,
    secret_signing_key: Arc<SecretKey>,
    pub(super) public_signing_key: PublicKey,
    current_era: EraId,
    protocol_config: ProtocolConfig,
    config: Config,
    #[data_size(skip)] // Negligible for most closures, zero for functions.
    new_consensus: Box<ConsensusConstructor<I>>,
    node_start_time: Timestamp,
    /// The unbonding period, in number of eras. After this many eras, a former validator is
    /// allowed to withdraw their stake, so their signature can't be trusted anymore.
    ///
    /// A node keeps `2 * bonded_eras` past eras around, because the oldest bonded era could still
    /// receive blocks that refer to `bonded_eras` before that.
    bonded_eras: u64,
    /// The height of the next block to be finalized.
    /// We keep that in order to be able to signal to the Block Proposer how many blocks have been
    /// finalized when we request a new block. This way the Block Proposer can know whether it's up
    /// to date, or whether it has to wait for more finalized blocks before responding.
    /// This value could be obtained from the consensus instance in a relevant era, but caching it
    /// here is the easiest way of achieving the desired effect.
    next_block_height: u64,
    #[data_size(skip)]
    metrics: ConsensusMetrics,
    // TODO: discuss this quick fix
    finished_joining: bool,
    /// The path to the folder where unit hash files will be stored.
    unit_hashes_folder: PathBuf,
    /// The next upgrade activation point.  When the era immediately before the activation point is
    /// deactivated, the era supervisor indicates that the node should stop running to allow an
    /// upgrade.
    next_upgrade_activation_point: Option<ActivationPoint>,
    /// If true, the process should stop execution to allow an upgrade to proceed.
    stop_for_upgrade: bool,
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
        validators: BTreeMap<PublicKey, U512>,
        protocol_config: ProtocolConfig,
        genesis_state_root_hash: Digest,
        registry: &Registry,
        new_consensus: Box<ConsensusConstructor<I>>,
        mut rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Event<I>>), Error> {
        let unit_hashes_folder = config.with_dir(config.value().unit_hashes_folder.clone());
        let (root, config) = config.into_parts();
        let secret_signing_key = Arc::new(config.secret_key_path.clone().load(root)?);
        let public_signing_key = PublicKey::from(secret_signing_key.as_ref());
        info!(our_id = %public_signing_key, "EraSupervisor pubkey",);
        let bonded_eras: u64 = protocol_config.unbonding_delay - protocol_config.auction_delay;
        let metrics = ConsensusMetrics::new(registry)
            .expect("failure to setup and register ConsensusMetrics");
        let genesis_start_time = protocol_config.timestamp;

        let mut era_supervisor = Self {
            active_eras: Default::default(),
            secret_signing_key,
            public_signing_key,
            current_era: EraId(0),
            protocol_config,
            config,
            new_consensus,
            node_start_time: Timestamp::now(),
            bonded_eras,
            next_block_height: 0,
            metrics,
            finished_joining: false,
            unit_hashes_folder,
            next_upgrade_activation_point: None,
            stop_for_upgrade: false,
        };

        let results = era_supervisor.new_era(
            EraId(0),
            timestamp,
            validators,
            vec![], // no banned validators in era 0
            0,      // hardcoded seed for era 0
            genesis_start_time,
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
        rng: &'a mut NodeRng,
    ) -> EraSupervisorHandlingWrapper<'a, I, REv> {
        EraSupervisorHandlingWrapper {
            era_supervisor: self,
            effect_builder,
            rng,
        }
    }

    fn booking_block_height(&self, era_id: EraId) -> u64 {
        // The booking block for era N is the last block of era N - AUCTION_DELAY - 1
        // To find it, we get the start height of era N - AUCTION_DELAY and subtract 1
        let after_booking_era_id =
            EraId(era_id.0.saturating_sub(self.protocol_config.auction_delay));
        self.active_eras
            .get(&after_booking_era_id)
            .expect("should have era after booking block")
            .start_height
            .saturating_sub(1)
    }

    fn era_seed(booking_block_hash: BlockHash, key_block_seed: Digest) -> u64 {
        let mut result = [0; Digest::LENGTH];
        let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");

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
        validators: BTreeMap<PublicKey, U512>,
        newly_slashed: Vec<PublicKey>,
        seed: u64,
        start_time: Timestamp,
        start_height: u64,
        state_root_hash: Digest,
    ) -> Vec<ProtocolOutcome<I, ClContext>> {
        if self.active_eras.contains_key(&era_id) {
            panic!("{} already exists", era_id);
        }
        self.current_era = era_id;
        self.metrics.current_era.set(self.current_era.0 as i64);
        let instance_id = instance_id(&self.protocol_config, state_root_hash);

        info!(
            ?validators,
            %start_time,
            %timestamp,
            %start_height,
            %instance_id,
            era = era_id.0,
            "starting era",
        );

        let slashed = era_id
            .iter_other(self.bonded_eras)
            .flat_map(|e_id| &self.active_eras[&e_id].newly_slashed)
            .chain(&newly_slashed)
            .cloned()
            .collect();

        // Activate the era if this node was already running when the era began, it is still
        // ongoing based on its minimum duration, and we are one of the validators.
        let our_id = self.public_signing_key;
        let should_activate = if !validators.contains_key(&our_id) {
            info!(era = era_id.0, %our_id, "not voting; not a validator");
            false
        } else if !self.finished_joining {
            info!(era = era_id.0, %our_id, "not voting; still joining");
            false
        } else {
            info!(era = era_id.0, %our_id, "start voting");
            true
        };

        let prev_era = era_id
            .checked_sub(1)
            .and_then(|last_era_id| self.active_eras.get(&last_era_id));

        let (mut consensus, mut outcomes) = (self.new_consensus)(
            instance_id,
            validators.clone(),
            &slashed,
            &self.protocol_config,
            &self.config,
            prev_era.map(|era| &*era.consensus),
            start_time,
            seed,
        );

        if should_activate {
            let secret = Keypair::new(self.secret_signing_key.clone(), our_id);
            let unit_hash_file = self.unit_hashes_folder.join(format!(
                "unit_hash_{:?}_{}.dat",
                instance_id,
                self.public_signing_key.to_hex()
            ));
            outcomes.extend(consensus.activate_validator(
                our_id,
                secret,
                timestamp,
                Some(unit_hash_file),
            ))
        }

        let era = Era::new(
            consensus,
            start_time,
            start_height,
            newly_slashed,
            slashed,
            validators,
        );
        let _ = self.active_eras.insert(era_id, era);

        // Remove the era that has become obsolete now. We keep 2 * bonded_eras past eras because
        // the oldest bonded era could still receive blocks that refer to bonded_eras before that.
        if let Some(obsolete_era_id) = era_id.checked_sub(2 * self.bonded_eras + 1) {
            trace!(era = obsolete_era_id.0, "removing obsolete era");
            self.active_eras.remove(&obsolete_era_id);
        }

        outcomes
    }

    /// Returns `true` if the specified era is active and bonded.
    fn is_bonded(&self, era_id: EraId) -> bool {
        era_id.0 + self.bonded_eras >= self.current_era.0 && era_id <= self.current_era
    }

    /// Returns whether the validator with the given public key is bonded in that era.
    fn is_validator_in(&self, pub_key: &PublicKey, era_id: EraId) -> bool {
        let has_validator = |era: &Era<I>| era.validators().contains_key(&pub_key);
        self.active_eras.get(&era_id).map_or(false, has_validator)
    }

    /// Inspect the active eras.
    #[cfg(test)]
    pub(crate) fn active_eras(&self) -> &HashMap<EraId, Era<I>> {
        &self.active_eras
    }

    /// To be called when we transition from the joiner to the validator reactor.
    pub(crate) fn finished_joining(
        &mut self,
        now: Timestamp,
    ) -> Vec<ProtocolOutcome<I, ClContext>> {
        self.finished_joining = true;
        let secret = Keypair::new(self.secret_signing_key.clone(), self.public_signing_key);
        let public_key = self.public_signing_key;
        let unit_hashes_folder = self.unit_hashes_folder.clone();
        self.active_eras
            .get_mut(&self.current_era)
            .map(|era| {
                if era.validators().contains_key(&public_key) {
                    let instance_id = *era.consensus.instance_id();
                    let unit_hash_file = unit_hashes_folder.join(format!(
                        "unit_hash_{:?}_{}.dat",
                        instance_id,
                        public_key.to_hex()
                    ));
                    era.consensus
                        .activate_validator(public_key, secret, now, Some(unit_hash_file))
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default()
    }

    pub(crate) fn stop_for_upgrade(&self) -> bool {
        self.stop_for_upgrade
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
    pub(super) rng: &'a mut NodeRng,
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
            &mut dyn ConsensusProtocol<I, ClContext>,
            &mut NodeRng,
        ) -> Vec<ProtocolOutcome<I, ClContext>>,
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
        timer_id: TimerId,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.handle_timer(timestamp, timer_id, rng)
        })
    }

    pub(super) fn handle_action(
        &mut self,
        era_id: EraId,
        action_id: ActionId,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.handle_action(action_id, rng)
        })
    }

    pub(super) fn handle_message(&mut self, sender: I, msg: ConsensusMessage) -> Effects<Event<I>> {
        match msg {
            ConsensusMessage::Protocol { era_id, payload } => {
                // If the era is already unbonded, only accept new evidence, because still-bonded
                // eras could depend on that.
                let evidence_only = !self.era_supervisor.is_bonded(era_id);
                trace!(era = era_id.0, "received a consensus message");
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
                    .iter_bonded(self.era_supervisor.bonded_eras)
                    .flat_map(|e_id| {
                        self.delegate_to_era(e_id, |consensus, _| {
                            consensus.request_evidence(sender.clone(), &pub_key)
                        })
                    })
                    .collect()
            }
        }
    }

    pub(super) fn handle_new_peer(&mut self, peer_id: I) -> Effects<Event<I>> {
        self.delegate_to_era(self.era_supervisor.current_era, move |consensus, _rng| {
            consensus.handle_new_peer(peer_id)
        })
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
        let accusations = era_id
            .iter_bonded(self.era_supervisor.bonded_eras)
            .flat_map(|e_id| self.era(e_id).consensus.validators_with_evidence())
            .unique()
            .filter(|pub_key| !self.era(era_id).slashed.contains(pub_key))
            .cloned()
            .collect();
        let candidate_block =
            CandidateBlock::new(proto_block, block_context.timestamp(), accusations);
        self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.propose(candidate_block, block_context, rng)
        })
    }

    pub(super) fn handle_linear_chain_block(
        &mut self,
        block_header: BlockHeader,
        responder: Responder<Option<FinalitySignature>>,
    ) -> Effects<Event<I>> {
        let our_pk = self.era_supervisor.public_signing_key;
        let our_sk = self.era_supervisor.secret_signing_key.clone();
        let era_id = block_header.era_id();
        let maybe_fin_sig = if self.era_supervisor.is_validator_in(&our_pk, era_id) {
            let block_hash = block_header.hash();
            Some(FinalitySignature::new(
                block_hash,
                era_id,
                &our_sk,
                our_pk,
                &mut self.rng,
            ))
        } else {
            None
        };
        let mut effects = responder.respond(maybe_fin_sig).ignore();
        if era_id < self.era_supervisor.current_era {
            trace!(era = era_id.0, "executed block in old era");
            return effects;
        }
        if block_header.switch_block() {
            // if the block is a switch block, we have to get the validators for the new era and
            // create it, before we can say we handled the block
            let new_era_id = era_id.successor();
            let booking_block_height = self.era_supervisor.booking_block_height(new_era_id);
            let effect = self
                .effect_builder
                .get_block_at_height_from_storage(booking_block_height)
                .event(move |booking_block| Event::CreateNewEra {
                    block_header: Box::new(block_header),
                    booking_block_hash: booking_block
                        .map_or_else(|| Err(booking_block_height), |block| Ok(*block.hash())),
                });
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

    pub(super) fn handle_deactivate_era(
        &mut self,
        era_id: EraId,
        old_faulty_num: usize,
        delay: Duration,
    ) -> Effects<Event<I>> {
        let era = if let Some(era) = self.era_supervisor.active_eras.get_mut(&era_id) {
            era
        } else {
            warn!(era = era_id.0, "trying to deactivate obsolete era");
            return Effects::new();
        };
        let faulty_num = era.consensus.validators_with_evidence().len();
        if faulty_num == old_faulty_num {
            info!(era = era_id.0, "stop voting in era");
            era.consensus.deactivate_validator();
            if let Some(upgrade_activation_point) =
                self.era_supervisor.next_upgrade_activation_point
            {
                // If the next era is at or after the upgrade activation point, stop the node.
                if upgrade_activation_point.should_upgrade(&era_id) {
                    info!(era = era_id.0, "shutting down for upgrade");
                    self.era_supervisor.stop_for_upgrade = true;
                }
            }
            Effects::new()
        } else {
            let deactivate_era = move |_| Event::DeactivateEra {
                era_id,
                faulty_num,
                delay,
            };
            self.effect_builder.set_timeout(delay).event(deactivate_era)
        }
    }

    pub(super) fn handle_create_new_era(
        &mut self,
        block_header: BlockHeader,
        booking_block_hash: BlockHash,
    ) -> Effects<Event<I>> {
        let (era_end, next_era_validators_weights) = match (
            block_header.era_end(),
            block_header.next_era_validator_weights(),
        ) {
            (Some(era_end), Some(next_era_validator_weights)) => {
                (era_end, next_era_validator_weights)
            }
            _ => {
                return fatal!(
                    self.effect_builder,
                    "attempted to create a new era with a non-switch block header: {}",
                    block_header
                )
            }
        };
        let newly_slashed = era_end.equivocators.clone();
        let era_id = block_header.era_id().successor();
        info!(era = era_id.0, "era created");
        let seed =
            EraSupervisor::<I>::era_seed(booking_block_hash, block_header.accumulated_seed());
        trace!(%seed, "the seed for {}: {}", era_id, seed);
        let results = self.era_supervisor.new_era(
            era_id,
            Timestamp::now(), // TODO: This should be passed in.
            next_era_validators_weights.clone(),
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
        sender: I,
        proto_block: ProtoBlock,
        timestamp: Timestamp,
        valid: bool,
    ) -> Effects<Event<I>> {
        self.era_supervisor.metrics.proposed_block();
        let mut effects = Effects::new();
        if !valid {
            warn!(
                %sender,
                era = %era_id.0,
                "invalid consensus value; disconnecting from the sender"
            );
            effects.extend(self.disconnect(sender));
        }
        let candidate_blocks = if let Some(era) = self.era_supervisor.active_eras.get_mut(&era_id) {
            era.resolve_validity(&proto_block, timestamp, valid)
        } else {
            return effects;
        };
        for candidate_block in candidate_blocks {
            effects.extend(self.delegate_to_era(era_id, |consensus, rng| {
                consensus.resolve_validity(&candidate_block, valid, rng)
            }));
        }
        effects
    }

    fn handle_consensus_results<T>(&mut self, era_id: EraId, results: T) -> Effects<Event<I>>
    where
        T: IntoIterator<Item = ProtocolOutcome<I, ClContext>>,
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
            .iter_bonded(self.era_supervisor.bonded_eras)
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
        consensus_result: ProtocolOutcome<I, ClContext>,
    ) -> Effects<Event<I>> {
        match consensus_result {
            ProtocolOutcome::InvalidIncomingMessage(_, sender, error) => {
                warn!(
                    %sender,
                    %error,
                    "invalid incoming message to consensus instance; disconnecting from the sender"
                );
                self.disconnect(sender)
            }
            ProtocolOutcome::Disconnect(sender) => {
                warn!(
                    %sender,
                    "disconnecting from the sender of invalid data"
                );
                self.disconnect(sender)
            }
            ProtocolOutcome::CreatedGossipMessage(out_msg) => {
                // TODO: we'll want to gossip instead of broadcast here
                self.effect_builder
                    .broadcast_message(era_id.message(out_msg).into())
                    .ignore()
            }
            ProtocolOutcome::CreatedTargetedMessage(out_msg, to) => self
                .effect_builder
                .send_message(to, era_id.message(out_msg).into())
                .ignore(),
            ProtocolOutcome::ScheduleTimer(timestamp, timer_id) => {
                let timediff = timestamp.saturating_diff(Timestamp::now());
                self.effect_builder
                    .set_timeout(timediff.into())
                    .event(move |_| Event::Timer {
                        era_id,
                        timestamp,
                        timer_id,
                    })
            }
            ProtocolOutcome::QueueAction(action_id) => self
                .effect_builder
                .immediately()
                .event(move |()| Event::Action { era_id, action_id }),
            ProtocolOutcome::CreateNewBlock {
                block_context,
                past_values,
            } => {
                let past_deploys = past_values
                    .iter()
                    .flat_map(|candidate| BlockLike::deploys(candidate.proto_block()))
                    .cloned()
                    .collect();
                self.effect_builder
                    .request_proto_block(
                        block_context,
                        past_deploys,
                        self.era_supervisor.next_block_height,
                        self.rng.gen(),
                    )
                    .event(move |(proto_block, block_context)| Event::NewProtoBlock {
                        era_id,
                        proto_block,
                        block_context,
                    })
            }
            ProtocolOutcome::FinalizedBlock(CpFinalizedBlock {
                value,
                timestamp,
                height,
                terminal_block_data,
                equivocators,
                proposer,
            }) => {
                let era = self.era_supervisor.active_eras.get_mut(&era_id).unwrap();
                era.add_accusations(&equivocators);
                era.add_accusations(value.accusations());
                // If this is the era's last block, it contains rewards. Everyone who is accused in
                // the block or seen as equivocating via the consensus protocol gets slashed.
                let era_end = terminal_block_data.map(|tbd| EraEnd {
                    rewards: tbd.rewards,
                    equivocators: era.accusations(),
                    inactive_validators: tbd.inactive_validators,
                    next_era_validator_weights: BTreeMap::default(),
                });
                let finalized_block = FinalizedBlock::new(
                    value.into(),
                    timestamp,
                    era_end,
                    era_id,
                    era.start_height + height,
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
                self.era_supervisor.next_block_height = finalized_block.height() + 1;
                if finalized_block.era_end().is_some() {
                    // This was the era's last block. Schedule deactivating this era.
                    let delay = Timestamp::now().saturating_diff(timestamp).into();
                    let faulty_num = era.consensus.validators_with_evidence().len();
                    let deactivate_era = move |_| Event::DeactivateEra {
                        era_id,
                        faulty_num,
                        delay,
                    };
                    effects.extend(self.effect_builder.set_timeout(delay).event(deactivate_era));
                }
                // Request execution of the finalized block.
                effects.extend(self.effect_builder.execute_block(finalized_block).ignore());
                effects
            }
            ProtocolOutcome::ValidateConsensusValue(sender, candidate_block, timestamp) => {
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
                        .validate_block(sender.clone(), proto_block, timestamp)
                        .event(move |(valid, proto_block)| Event::ResolveValidity {
                            era_id,
                            sender,
                            proto_block,
                            timestamp,
                            valid,
                        }),
                );
                effects
            }
            ProtocolOutcome::NewEvidence(pub_key) => {
                info!(%pub_key, era = era_id.0, "validator equivocated");
                let mut effects = self
                    .effect_builder
                    .announce_fault_event(era_id, pub_key, Timestamp::now())
                    .ignore();
                for e_id in (era_id.0..=(era_id.0 + self.era_supervisor.bonded_eras)).map(EraId) {
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
            ProtocolOutcome::SendEvidence(sender, pub_key) => era_id
                .iter_other(self.era_supervisor.bonded_eras)
                .flat_map(|e_id| {
                    self.delegate_to_era(e_id, |consensus, _| {
                        consensus.request_evidence(sender.clone(), &pub_key)
                    })
                })
                .collect(),
            ProtocolOutcome::WeAreFaulty => Default::default(),
            ProtocolOutcome::DoppelgangerDetected => Default::default(),
        }
    }

    /// Emits a fatal error if the consensus state is still empty.
    pub(super) fn shutdown_if_necessary(&self) -> Effects<Event<I>> {
        let should_emit_error = self
            .era_supervisor
            .active_eras
            .iter()
            .all(|(_, era)| !era.consensus.has_received_messages());
        if should_emit_error {
            fatal!(
                self.effect_builder,
                "Consensus shutting down due to inability to participate in the network; inactive era = {}",
                self.era_supervisor.current_era
            )
        } else {
            Default::default()
        }
    }

    pub(crate) fn finished_joining(&mut self, now: Timestamp) -> Effects<Event<I>> {
        let results = self.era_supervisor.finished_joining(now);
        self.handle_consensus_results(self.era_supervisor.current_era, results)
    }

    /// Handles registering an upgrade activation point.
    pub(super) fn got_upgrade_activation_point(
        &mut self,
        activation_point: ActivationPoint,
    ) -> Effects<Event<I>> {
        debug!("got {}", activation_point);
        self.era_supervisor.next_upgrade_activation_point = Some(activation_point);
        Effects::new()
    }

    /// Returns whether validator is bonded in an era.
    pub(super) fn is_bonded_validator(
        &self,
        era_id: EraId,
        vid: PublicKey,
        responder: Responder<bool>,
    ) -> Effects<Event<I>> {
        let is_bonded = self
            .era_supervisor
            .active_eras
            .get(&era_id)
            .map_or(false, |cp| cp.is_bonded_validator(&vid));
        responder.respond(is_bonded).ignore()
    }

    fn disconnect(&self, sender: I) -> Effects<Event<I>> {
        self.effect_builder
            .announce_disconnect_from_peer(sender)
            .ignore()
    }
}

/// Computes the instance ID for an era, given the state root hash, block height and chainspec.
fn instance_id(protocol_config: &ProtocolConfig, state_root_hash: Digest) -> Digest {
    let mut result = [0; Digest::LENGTH];
    let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");

    hasher.update(&protocol_config.name);
    hasher.update(protocol_config.timestamp.millis().to_le_bytes());
    hasher.update(state_root_hash);
    hasher.update(protocol_config.protocol_version.to_string().as_bytes());

    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    result.into()
}
