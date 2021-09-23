//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

mod era;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    fs, io,
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
use futures::FutureExt;
use itertools::Itertools;
use prometheus::Registry;
use rand::Rng;
use tracing::{debug, error, info, trace, warn};

use casper_types::{AsymmetricType, EraId, PublicKey, SecretKey, U512};

use crate::{
    components::{
        consensus::{
            cl_context::{ClContext, Keypair},
            config::ProtocolConfig,
            consensus_protocol::{
                ConsensusProtocol, EraReport, FinalizedBlock as CpFinalizedBlock, ProposedBlock,
                ProtocolOutcome,
            },
            metrics::ConsensusMetrics,
            traits::NodeIdT,
            ActionId, Config, ConsensusMessage, Event, NewBlockPayload, ReactorEventT,
            ResolveValidity, TimerId,
        },
        storage::Storage,
    },
    crypto::hash::Digest,
    effect::{
        announcements::ControlAnnouncement,
        requests::{BlockValidationRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::{
        ActivationPoint, BlockHash, BlockHeader, Deploy, DeployHash, DeployOrTransferHash,
        FinalitySignature, FinalizedBlock, TimeDiff, Timestamp,
    },
    utils::WithDir,
    NodeRng,
};

pub use self::era::Era;

/// The delay in milliseconds before we shutdown after the number of faulty validators exceeded the
/// fault tolerance threshold.
const FTT_EXCEEDED_SHUTDOWN_DELAY_MILLIS: u64 = 60 * 1000;

type ConsensusConstructor<I> = dyn Fn(
        Digest,                    // the era's unique instance ID
        BTreeMap<PublicKey, U512>, // validator weights
        &HashSet<PublicKey>,       /* faulty validators that are banned in
                                    * this era */
        &HashSet<PublicKey>, // inactive validators that can't be leaders
        &ProtocolConfig,     // the network's chainspec
        &Config,             // The consensus part of the node config.
        Option<&dyn ConsensusProtocol<I, ClContext>>, // previous era's consensus instance
        Timestamp,           // start time for this era
        u64,                 // random seed
        Timestamp,           // now timestamp
    ) -> (
        Box<dyn ConsensusProtocol<I, ClContext>>,
        Vec<ProtocolOutcome<I, ClContext>>,
    ) + Send;

#[derive(DataSize)]
pub struct EraSupervisor<I> {
    /// A map of active consensus protocols.
    /// A value is a trait so that we can run different consensus protocol instances per era.
    ///
    /// This map always contains exactly three entries, with the last one being the current one.
    active_eras: HashMap<EraId, Era<I>>,
    secret_signing_key: Arc<SecretKey>,
    public_signing_key: PublicKey,
    current_era: EraId,
    protocol_config: ProtocolConfig,
    config: Config,
    #[data_size(skip)] // Negligible for most closures, zero for functions.
    new_consensus: Box<ConsensusConstructor<I>>,
    /// The height of the next block to be finalized.
    /// We keep that in order to be able to signal to the Block Proposer how many blocks have been
    /// finalized when we request a new block. This way the Block Proposer can know whether it's up
    /// to date, or whether it has to wait for more finalized blocks before responding.
    /// This value could be obtained from the consensus instance in a relevant era, but caching it
    /// here is the easiest way of achieving the desired effect.
    next_block_height: u64,
    /// The height of the next block to be executed. If this falls too far behind, we pause.
    next_executed_height: u64,
    #[data_size(skip)]
    metrics: ConsensusMetrics,
    /// The path to the folder where unit hash files will be stored.
    unit_hashes_folder: PathBuf,
    /// The next upgrade activation point. When the era immediately before the activation point is
    /// deactivated, the era supervisor indicates that the node should stop running to allow an
    /// upgrade.
    next_upgrade_activation_point: Option<ActivationPoint>,
    /// If true, the process should stop execution to allow an upgrade to proceed.
    stop_for_upgrade: bool,
    /// The era that was current when this node joined the network.
    era_where_we_joined: EraId,
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
    /// Creates a new `EraSupervisor`, starting in the indicated current era.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new<REv: ReactorEventT<I>>(
        current_era: EraId,
        config: WithDir<Config>,
        effect_builder: EffectBuilder<REv>,
        protocol_config: ProtocolConfig,
        latest_block_header: &BlockHeader,
        next_upgrade_activation_point: Option<ActivationPoint>,
        registry: &Registry,
        new_consensus: Box<ConsensusConstructor<I>>,
        storage: &Storage,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Event<I>>), Error> {
        if current_era <= protocol_config.last_activation_point {
            panic!(
                "Current era ({:?}) is before the last activation point ({:?}) - no eras would \
                be instantiated!",
                current_era, protocol_config.last_activation_point
            );
        }
        let unit_hashes_folder = config.with_dir(config.value().highway.unit_hashes_folder.clone());
        let (root, config) = config.into_parts();
        let (secret_signing_key, public_signing_key) = config.load_keys(root)?;
        info!(our_id = %public_signing_key, "EraSupervisor pubkey",);
        let metrics = ConsensusMetrics::new(registry)
            .expect("failure to setup and register ConsensusMetrics");
        #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
        let next_height = latest_block_header.height() + 1;

        let mut era_supervisor = Self {
            active_eras: Default::default(),
            secret_signing_key,
            public_signing_key,
            current_era,
            protocol_config,
            config,
            new_consensus,
            next_block_height: next_height,
            metrics,
            unit_hashes_folder,
            next_upgrade_activation_point,
            stop_for_upgrade: false,
            next_executed_height: next_height,
            era_where_we_joined: current_era,
        };

        // Collect the information needed to initialize all recent eras.
        let auction_delay = era_supervisor.protocol_config.auction_delay;
        let last_activation_point = era_supervisor.protocol_config.last_activation_point;
        let mut switch_blocks = Vec::new();
        // We need to initialize current_era, current_era - 1 and current_era - 2. To initialize an
        // era, all switch blocks between its booking block and its key block are required. The
        // booking block for era N is in N - auction_delay - 1, and the key block in N - 1.
        // So we need all switch blocks between (including) current_era - 2 - auction_delay - 1
        // and (excluding) current_era. However, we never use any block from before the last
        // activation point.
        let earliest_era =
            last_activation_point.max(current_era.saturating_sub(auction_delay).saturating_sub(3));
        for era_id in (earliest_era.value()..current_era.value()).map(EraId::from) {
            let switch_block = storage
                .read_switch_block_header_by_era_id(era_id)?
                .ok_or_else(|| anyhow::Error::msg("No such switch block"))?;
            switch_blocks.push(switch_block);
        }
        // The create_new_era method initializes the era that the slice's last block is the key
        // block for. We want to initialize the three latest eras, so we have to pass in the whole
        // slice for the current era, and omit one or two elements for the other two. We never
        // initialize the last_activation_point or an earlier era, however.
        let mut effects = Effects::new();
        for i in (switch_blocks.len().saturating_sub(2).max(1)..=switch_blocks.len()).rev() {
            effects.extend(era_supervisor.create_new_era(effect_builder, rng, &switch_blocks[..i]));
        }
        Ok((era_supervisor, effects))
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

    /// Returns an iterator over era IDs of `num_eras` past eras, plus the provided one.
    ///
    /// Note: Excludes the activation point era and earlier eras. The activation point era itself
    /// contains only the single switch block we created after the upgrade. There is no consensus
    /// instance for it.
    pub(crate) fn iter_past(&self, era_id: EraId, num_eras: u64) -> impl Iterator<Item = EraId> {
        (self
            .protocol_config
            .last_activation_point
            .successor()
            .max(era_id.saturating_sub(num_eras))
            .value()..=era_id.value())
            .map(EraId::from)
    }

    /// Returns an iterator over era IDs of `num_eras` past eras, excluding the provided one.
    ///
    /// Note: Excludes the activation point era and earlier eras. The activation point era itself
    /// contains only the single switch block we created after the upgrade. There is no consensus
    /// instance for it.
    pub(crate) fn iter_past_other(
        &self,
        era_id: EraId,
        num_eras: u64,
    ) -> impl Iterator<Item = EraId> {
        (self
            .protocol_config
            .last_activation_point
            .successor()
            .max(era_id.saturating_sub(num_eras))
            .value()..era_id.value())
            .map(EraId::from)
    }

    /// Returns an iterator over era IDs of `num_eras` future eras, plus the provided one.
    fn iter_future(&self, era_id: EraId, num_eras: u64) -> impl Iterator<Item = EraId> {
        (era_id.value()..=era_id.value().saturating_add(num_eras)).map(EraId::from)
    }

    /// Returns whether the validator with the given public key is bonded in that era.
    fn is_validator_in(&self, pub_key: &PublicKey, era_id: EraId) -> bool {
        let has_validator = |era: &Era<I>| era.validators().contains_key(pub_key);
        self.active_eras.get(&era_id).map_or(false, has_validator)
    }

    pub(crate) fn stop_for_upgrade(&self) -> bool {
        self.stop_for_upgrade
    }

    /// Updates `next_executed_height` based on the given block header, and unpauses consensus if
    /// block execution has caught up with finalization.
    #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
    fn executed_block(&mut self, block_header: &BlockHeader) {
        self.next_executed_height = self.next_executed_height.max(block_header.height() + 1);
        self.update_consensus_pause();
    }

    /// Pauses or unpauses consensus: Whenever the last executed block is too far behind the last
    /// finalized block, we suspend consensus.
    fn update_consensus_pause(&mut self) {
        let paused = self
            .next_block_height
            .saturating_sub(self.next_executed_height)
            > self.config.highway.max_execution_delay;
        match self.active_eras.get_mut(&self.current_era) {
            Some(era) => era.set_paused(paused),
            None => error!(
                era = self.current_era.value(),
                "current era not initialized"
            ),
        }
    }

    /// Initializes a new era. The switch blocks must contain the most recent `auction_delay + 1`
    /// ones, in order, but at most as far back as to the last activation point.
    pub(super) fn create_new_era<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        switch_blocks: &[BlockHeader],
    ) -> Effects<Event<I>> {
        let key_block = if let Some(key_block) = switch_blocks.last() {
            key_block
        } else {
            return fatal!(
                effect_builder,
                "attempted to create era with no switch blocks; this is a bug",
            )
            .ignore();
        };

        let era_id = key_block.era_id().successor();

        let (report, validators) = match (
            key_block.era_report(),
            key_block.next_era_validator_weights(),
        ) {
            (Some(report), Some(validators)) => (report, validators.clone()),
            (_, _) => {
                return fatal!(
                    effect_builder,
                    "attempted to create {} with non-switch block {}; this is a bug",
                    era_id,
                    key_block,
                )
                .ignore();
            }
        };

        if self.active_eras.contains_key(&era_id) {
            warn!(era = era_id.value(), "era already exists");
            return Effects::new();
        }
        if self.current_era > era_id.saturating_add(2) {
            warn!(era = era_id.value(), "trying to create obsolete era");
            return Effects::new();
        }

        // Compute the seed for the PRNG from the booking block hash and the accumulated seed.
        let auction_delay = self.protocol_config.auction_delay as usize;
        let booking_block_hash =
            if let Some(booking_block) = switch_blocks.iter().rev().nth(auction_delay) {
                booking_block.hash()
            } else {
                // If there's no booking block for the `era_id`
                // (b/c it would have been from before Genesis, upgrade or emergency restart),
                // use a "zero" block hash. This should not hurt the security of the leader
                // selection algorithm.
                BlockHash::default()
            };
        let seed = Self::era_seed(booking_block_hash, key_block.accumulated_seed());

        // The beginning of the new era is marked by the key block.
        #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
        let start_height = key_block.height() + 1;
        let start_time = key_block.timestamp();

        // Validators that were inactive in the previous era will be excluded from leader selection
        // in the new era.
        let inactive = report.inactive_validators.iter().cloned().collect();

        // Validators that were only exposed as faulty after the booking block are still in the new
        // era's validator set but get banned.
        let blocks_after_booking_block = switch_blocks.iter().rev().take(auction_delay);
        let faulty = blocks_after_booking_block
            .filter_map(|switch_block| switch_block.era_report())
            .flat_map(|report| report.equivocators.clone())
            .collect();

        let instance_id = instance_id(&self.protocol_config, era_id);
        let now = Timestamp::now();

        info!(
            ?validators,
            %start_time,
            %now,
            %start_height,
            %instance_id,
            %seed,
            era = era_id.value(),
            "starting era",
        );

        let maybe_prev_era = era_id
            .checked_sub(1)
            .and_then(|last_era_id| self.active_eras.get(&last_era_id));
        let validators_with_evidence: Vec<PublicKey> = maybe_prev_era
            .into_iter()
            .flat_map(|prev_era| prev_era.consensus.validators_with_evidence())
            .cloned()
            .collect();

        // Create and insert the new era instance.
        let (consensus, mut outcomes) = (self.new_consensus)(
            instance_id,
            validators.clone(),
            &faulty,
            &inactive,
            &self.protocol_config,
            &self.config,
            maybe_prev_era.map(|prev_era| &*prev_era.consensus),
            start_time,
            seed,
            now,
        );
        let era = Era::new(consensus, start_time, start_height, faulty, validators);
        let _ = self.active_eras.insert(era_id, era);

        // Activate the era if this node was already running when the era began, it is still
        // ongoing based on its minimum duration, and we are one of the validators.
        let our_id = self.public_signing_key.clone();
        if self.current_era > era_id {
            trace!(
                era = era_id.value(),
                current_era = self.current_era.value(),
                "not voting; initializing past era"
            );
        } else {
            self.current_era = era_id;
            self.metrics.current_era.set(era_id.value() as i64);
            self.next_block_height = self.next_block_height.max(start_height);
            outcomes.extend(self.era_mut(era_id).consensus.handle_is_current());
            if !self.era(era_id).validators().contains_key(&our_id) {
                info!(era = era_id.value(), %our_id, "not voting; not a validator");
            } else {
                info!(era = era_id.value(), %our_id, "start voting");
                let secret = Keypair::new(self.secret_signing_key.clone(), our_id.clone());
                let unit_hash_file = self.unit_hash_file(&instance_id);
                outcomes.extend(self.era_mut(era_id).consensus.activate_validator(
                    our_id,
                    secret,
                    now,
                    Some(unit_hash_file),
                ))
            };
        }

        // Mark validators as faulty for which we have evidence in the previous era.
        for pub_key in validators_with_evidence {
            let proposed_blocks = self
                .era_mut(era_id)
                .resolve_evidence_and_mark_faulty(&pub_key);
            if !proposed_blocks.is_empty() {
                error!(
                    ?proposed_blocks,
                    era = era_id.value(),
                    "unexpected block in new era"
                );
            }
        }

        // Clear the obsolete data from the era before the previous one. We only retain the
        // information necessary to validate evidence that units in the two most recent eras may
        // refer to for cross-era fault tracking.
        if let Some(evidence_only_era_id) = self.current_era.checked_sub(2) {
            if let Some(era) = self.active_eras.get_mut(&evidence_only_era_id) {
                trace!(era = evidence_only_era_id.value(), "clearing unbonded era");
                era.consensus.set_evidence_only();
            }
        }

        // Remove the era that has become obsolete now: We keep only three in memory.
        if let Some(obsolete_era_id) = self.current_era.checked_sub(3) {
            if let Some(era) = self.active_eras.remove(&obsolete_era_id) {
                trace!(era = obsolete_era_id.value(), "removing obsolete era");
                match fs::remove_file(self.unit_hash_file(era.consensus.instance_id())) {
                    Ok(_) => {}
                    Err(err) => match err.kind() {
                        io::ErrorKind::NotFound => {}
                        err => warn!(?err, "could not delete unit hash file"),
                    },
                }
            }
        }

        self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
    }

    /// Returns the path to the era's unit hash file.
    fn unit_hash_file(&self, instance_id: &Digest) -> PathBuf {
        self.unit_hashes_folder.join(format!(
            "unit_hash_{:?}_{}.dat",
            instance_id,
            self.public_signing_key.to_hex()
        ))
    }

    /// Applies `f` to the consensus protocol of the specified era.
    fn delegate_to_era<REv: ReactorEventT<I>, F>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        f: F,
    ) -> Effects<Event<I>>
    where
        F: FnOnce(&mut dyn ConsensusProtocol<I, ClContext>) -> Vec<ProtocolOutcome<I, ClContext>>,
    {
        match self.active_eras.get_mut(&era_id) {
            None => {
                if era_id > self.current_era {
                    info!(era = era_id.value(), "received message for future era");
                } else {
                    info!(era = era_id.value(), "received message for obsolete era");
                }
                Effects::new()
            }
            Some(era) => {
                let outcomes = f(&mut *era.consensus);
                self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
            }
        }
    }

    pub(super) fn handle_timer<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        timestamp: Timestamp,
        timer_id: TimerId,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus| {
            consensus.handle_timer(timestamp, timer_id)
        })
    }

    pub(super) fn handle_action<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        action_id: ActionId,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus| {
            consensus.handle_action(action_id, Timestamp::now())
        })
    }

    pub(super) fn handle_message<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        sender: I,
        msg: ConsensusMessage,
    ) -> Effects<Event<I>> {
        match msg {
            ConsensusMessage::Protocol { era_id, payload } => {
                // If the era is already unbonded, only accept new evidence, because still-bonded
                // eras could depend on that.
                trace!(era = era_id.value(), "received a consensus message");
                self.delegate_to_era(effect_builder, rng, era_id, move |consensus| {
                    consensus.handle_message(sender, payload, Timestamp::now())
                })
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => {
                if !self.active_eras.contains_key(&era_id) || era_id.successor() < self.current_era
                {
                    trace!(era = era_id.value(), "not handling message; era too old");
                    return Effects::new();
                }
                self.iter_past(era_id, 1)
                    .flat_map(|e_id| {
                        self.delegate_to_era(effect_builder, rng, e_id, |consensus| {
                            consensus.request_evidence(sender.clone(), &pub_key)
                        })
                    })
                    .collect()
            }
        }
    }

    pub(super) fn handle_new_block_payload<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        new_block_payload: NewBlockPayload,
    ) -> Effects<Event<I>> {
        let NewBlockPayload {
            era_id,
            block_payload,
            block_context,
        } = new_block_payload;
        if !self.active_eras.contains_key(&era_id) || era_id.successor() < self.current_era {
            warn!(era = era_id.value(), "new block payload in outdated era");
            return Effects::new();
        }
        let proposed_block = ProposedBlock::new(block_payload, block_context);
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus| {
            consensus.propose(proposed_block, Timestamp::now())
        })
    }

    pub(super) fn handle_block_added<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_header: BlockHeader,
    ) -> Effects<Event<I>> {
        let our_pk = self.public_signing_key.clone();
        let our_sk = self.secret_signing_key.clone();
        let era_id = block_header.era_id();
        self.executed_block(&block_header);
        let mut effects = if self.is_validator_in(&our_pk, era_id) {
            effect_builder
                .announce_created_finality_signature(FinalitySignature::new(
                    block_header.hash(),
                    era_id,
                    &our_sk,
                    our_pk,
                ))
                .ignore()
        } else {
            Effects::new()
        };
        if era_id < self.current_era {
            trace!(era = era_id.value(), "executed block in old era");
            return effects;
        }
        if block_header.is_switch_block() && !self.should_upgrade_after(&era_id) {
            // if the block is a switch block, we have to get the validators for the new era and
            // create it, before we can say we handled the block
            let new_era_id = era_id.successor();
            let effect = get_switch_blocks(
                effect_builder,
                new_era_id,
                self.protocol_config.auction_delay,
                self.protocol_config.last_activation_point,
            )
            .event(move |switch_blocks| Event::CreateNewEra { switch_blocks });
            effects.extend(effect);
        }
        effects
    }

    pub(super) fn handle_deactivate_era<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        era_id: EraId,
        old_faulty_num: usize,
        delay: Duration,
    ) -> Effects<Event<I>> {
        let era = if let Some(era) = self.active_eras.get_mut(&era_id) {
            era
        } else {
            warn!(era = era_id.value(), "trying to deactivate obsolete era");
            return Effects::new();
        };
        let faulty_num = era.consensus.validators_with_evidence().len();
        if faulty_num == old_faulty_num {
            info!(era = era_id.value(), "stop voting in era");
            era.consensus.deactivate_validator();
            if self.should_upgrade_after(&era_id) {
                // If the next era is at or after the upgrade activation point, stop the node.
                info!(era = era_id.value(), "shutting down for upgrade");
                self.stop_for_upgrade = true;
            }
            Effects::new()
        } else {
            let deactivate_era = move |_| Event::DeactivateEra {
                era_id,
                faulty_num,
                delay,
            };
            effect_builder.set_timeout(delay).event(deactivate_era)
        }
    }

    pub(super) fn resolve_validity<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        resolve_validity: ResolveValidity<I>,
    ) -> Effects<Event<I>> {
        let ResolveValidity {
            era_id,
            sender,
            proposed_block,
            valid,
        } = resolve_validity;
        self.metrics.proposed_block();
        let mut effects = Effects::new();
        if !valid {
            warn!(
                %sender,
                era = %era_id.value(),
                "invalid consensus value; disconnecting from the sender"
            );
            effects.extend(self.disconnect(effect_builder, sender));
        }
        if self
            .active_eras
            .get_mut(&era_id)
            .map_or(false, |era| era.resolve_validity(&proposed_block, valid))
        {
            effects.extend(
                self.delegate_to_era(effect_builder, rng, era_id, |consensus| {
                    consensus.resolve_validity(proposed_block, valid, Timestamp::now())
                }),
            );
        }
        effects
    }

    fn handle_consensus_outcomes<REv: ReactorEventT<I>, T>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        outcomes: T,
    ) -> Effects<Event<I>>
    where
        T: IntoIterator<Item = ProtocolOutcome<I, ClContext>>,
    {
        outcomes
            .into_iter()
            .flat_map(|result| self.handle_consensus_outcome(effect_builder, rng, era_id, result))
            .collect()
    }

    /// Returns `true` if any of the most recent eras has evidence against the validator with key
    /// `pub_key`.
    fn has_evidence(&self, era_id: EraId, pub_key: PublicKey) -> bool {
        self.iter_past(era_id, 1)
            .any(|eid| self.era(eid).consensus.has_evidence(&pub_key))
    }

    /// Returns the era with the specified ID. Panics if it does not exist.
    fn era(&self, era_id: EraId) -> &Era<I> {
        &self.active_eras[&era_id]
    }

    /// Returns the era with the specified ID mutably. Panics if it does not exist.
    fn era_mut(&mut self, era_id: EraId) -> &mut Era<I> {
        self.active_eras.get_mut(&era_id).unwrap()
    }

    #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
    fn handle_consensus_outcome<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
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
                self.disconnect(effect_builder, sender)
            }
            ProtocolOutcome::Disconnect(sender) => {
                warn!(
                    %sender,
                    "disconnecting from the sender of invalid data"
                );
                self.disconnect(effect_builder, sender)
            }
            ProtocolOutcome::CreatedGossipMessage(payload) => {
                let message = ConsensusMessage::Protocol { era_id, payload };
                // TODO: we'll want to gossip instead of broadcast here
                effect_builder.broadcast_message(message.into()).ignore()
            }
            ProtocolOutcome::CreatedTargetedMessage(payload, to) => {
                let message = ConsensusMessage::Protocol { era_id, payload };
                effect_builder.send_message(to, message.into()).ignore()
            }
            ProtocolOutcome::ScheduleTimer(timestamp, timer_id) => {
                let timediff = timestamp.saturating_diff(Timestamp::now());
                effect_builder
                    .set_timeout(timediff.into())
                    .event(move |_| Event::Timer {
                        era_id,
                        timestamp,
                        timer_id,
                    })
            }
            ProtocolOutcome::QueueAction(action_id) => effect_builder
                .immediately()
                .event(move |()| Event::Action { era_id, action_id }),
            ProtocolOutcome::CreateNewBlock(block_context) => {
                let accusations = self
                    .iter_past(era_id, 1)
                    .flat_map(|e_id| self.era(e_id).consensus.validators_with_evidence())
                    .unique()
                    .filter(|pub_key| !self.era(era_id).faulty.contains(pub_key))
                    .cloned()
                    .collect();
                effect_builder
                    .request_block_payload(
                        block_context.clone(),
                        self.next_block_height,
                        accusations,
                        rng.gen(),
                    )
                    .event(move |block_payload| {
                        Event::NewBlockPayload(NewBlockPayload {
                            era_id,
                            block_payload,
                            block_context,
                        })
                    })
            }
            ProtocolOutcome::FinalizedBlock(CpFinalizedBlock {
                value,
                timestamp,
                relative_height,
                terminal_block_data,
                equivocators,
                proposer,
            }) => {
                if era_id != self.current_era {
                    debug!(era = era_id.value(), "finalized block in old era");
                    return Effects::new();
                }
                let era = self.active_eras.get_mut(&era_id).unwrap();
                era.add_accusations(&equivocators);
                era.add_accusations(value.accusations());
                // If this is the era's last block, it contains rewards. Everyone who is accused in
                // the block or seen as equivocating via the consensus protocol gets faulty.
                let report = terminal_block_data.map(|tbd| EraReport {
                    rewards: tbd.rewards,
                    equivocators: era.accusations(),
                    inactive_validators: tbd.inactive_validators,
                });
                let finalized_block = FinalizedBlock::new(
                    Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone()),
                    report,
                    timestamp,
                    era_id,
                    era.start_height + relative_height,
                    proposer,
                );
                info!(?finalized_block, "finalized block");
                self.metrics.finalized_block(&finalized_block);
                // Announce the finalized block.
                let mut effects = effect_builder
                    .announce_finalized_block(finalized_block.clone())
                    .ignore();
                self.next_block_height = self.next_block_height.max(finalized_block.height() + 1);
                if finalized_block.era_report().is_some() {
                    // This was the era's last block. Schedule deactivating this era.
                    let delay = Timestamp::now().saturating_diff(timestamp).into();
                    let faulty_num = era.consensus.validators_with_evidence().len();
                    let deactivate_era = move |_| Event::DeactivateEra {
                        era_id,
                        faulty_num,
                        delay,
                    };
                    effects.extend(effect_builder.set_timeout(delay).event(deactivate_era));
                }
                // Request execution of the finalized block.
                effects.extend(execute_finalized_block(effect_builder, finalized_block).ignore());
                self.update_consensus_pause();
                effects
            }
            ProtocolOutcome::ValidateConsensusValue {
                sender,
                proposed_block,
            } => {
                if !self.active_eras.contains_key(&era_id) || era_id.successor() < self.current_era
                {
                    return Effects::new(); // Outdated era; we don't need the value anymore.
                }
                let missing_evidence: Vec<PublicKey> = proposed_block
                    .value()
                    .accusations()
                    .iter()
                    .filter(|pub_key| !self.has_evidence(era_id, (*pub_key).clone()))
                    .cloned()
                    .collect();
                self.era_mut(era_id)
                    .add_block(proposed_block.clone(), missing_evidence.clone());
                if let Some(deploy_hash) = proposed_block.contains_replay() {
                    info!(%sender, %deploy_hash, "block contains a replayed deploy");
                    return self.resolve_validity(
                        effect_builder,
                        rng,
                        ResolveValidity {
                            era_id,
                            sender,
                            proposed_block,
                            valid: false,
                        },
                    );
                }
                let mut effects = Effects::new();
                for pub_key in missing_evidence {
                    let msg = ConsensusMessage::EvidenceRequest { era_id, pub_key };
                    effects.extend(
                        effect_builder
                            .send_message(sender.clone(), msg.into())
                            .ignore(),
                    );
                }
                effects.extend(
                    async move {
                        check_deploys_for_replay_in_previous_eras_and_validate_block(
                            effect_builder,
                            era_id,
                            sender,
                            proposed_block,
                        )
                        .await
                    }
                    .event(std::convert::identity),
                );
                effects
            }
            ProtocolOutcome::NewEvidence(pub_key) => {
                info!(%pub_key, era = era_id.value(), "validator equivocated");
                let mut effects = effect_builder
                    .announce_fault_event(era_id, pub_key.clone(), Timestamp::now())
                    .ignore();
                for e_id in self.iter_future(era_id, 1) {
                    let proposed_blocks = if let Some(era) = self.active_eras.get_mut(&e_id) {
                        era.resolve_evidence_and_mark_faulty(&pub_key)
                    } else {
                        continue;
                    };
                    for proposed_block in proposed_blocks {
                        effects.extend(self.delegate_to_era(
                            effect_builder,
                            rng,
                            e_id,
                            |consensus| {
                                consensus.resolve_validity(proposed_block, true, Timestamp::now())
                            },
                        ));
                    }
                }
                effects
            }
            ProtocolOutcome::SendEvidence(sender, pub_key) => self
                .iter_past_other(era_id, 1)
                .flat_map(|e_id| {
                    self.delegate_to_era(effect_builder, rng, e_id, |consensus| {
                        consensus.request_evidence(sender.clone(), &pub_key)
                    })
                })
                .collect(),
            ProtocolOutcome::WeAreFaulty => Default::default(),
            ProtocolOutcome::DoppelgangerDetected => Default::default(),
            ProtocolOutcome::FttExceeded => effect_builder
                .set_timeout(Duration::from_millis(FTT_EXCEEDED_SHUTDOWN_DELAY_MILLIS))
                .then(move |_| fatal!(effect_builder, "too many faulty validators"))
                .ignore(),
            ProtocolOutcome::StandstillAlert => {
                if era_id == self.current_era && era_id == self.era_where_we_joined {
                    warn!(era = %era_id.value(), "current era is stalled; shutting down");
                    fatal!(effect_builder, "current era is stalled; please retry").ignore()
                } else {
                    if era_id == self.current_era {
                        warn!(era = %era_id.value(), "current era is stalled");
                    }
                    Effects::new()
                }
            }
        }
    }

    /// Handles registering an upgrade activation point.
    pub(super) fn got_upgrade_activation_point(
        &mut self,
        activation_point: ActivationPoint,
    ) -> Effects<Event<I>> {
        debug!("got {}", activation_point);
        self.next_upgrade_activation_point = Some(activation_point);
        Effects::new()
    }

    pub(super) fn status(
        &self,
        responder: Responder<Option<(PublicKey, Option<TimeDiff>)>>,
    ) -> Effects<Event<I>> {
        let public_key = self.public_signing_key.clone();
        let round_length = self
            .active_eras
            .get(&self.current_era)
            .and_then(|era| era.consensus.next_round_length());
        responder.respond(Some((public_key, round_length))).ignore()
    }

    fn disconnect<REv: ReactorEventT<I>>(
        &self,
        effect_builder: EffectBuilder<REv>,
        sender: I,
    ) -> Effects<Event<I>> {
        effect_builder
            .announce_disconnect_from_peer(sender)
            .ignore()
    }

    pub(super) fn should_upgrade_after(&self, era_id: &EraId) -> bool {
        match self.next_upgrade_activation_point {
            None => false,
            Some(upgrade_point) => upgrade_point.should_upgrade(era_id),
        }
    }
}

#[cfg(test)]
impl<I> EraSupervisor<I>
where
    I: NodeIdT,
{
    /// Returns the most recent active era.
    pub(crate) fn current_era(&self) -> EraId {
        self.current_era
    }

    /// Returns the list of validators who equivocated in this era.
    pub(crate) fn validators_with_evidence(&self, era_id: EraId) -> Vec<&PublicKey> {
        self.active_eras[&era_id]
            .consensus
            .validators_with_evidence()
    }

    /// Returns an iterator over all eras that are currently instantiated.
    pub(crate) fn active_era_ids(&self) -> impl Iterator<Item = &EraId> {
        self.active_eras.keys()
    }
}

/// Returns all switch blocks needed to initialize `era_id`.
///
/// Those are the booking block, i.e. the switch block in `era_id - auction_delay - 1`,
/// the key block, i.e. the switch block in `era_id - 1`, and all switch blocks in between.
async fn get_switch_blocks<REv>(
    effect_builder: EffectBuilder<REv>,
    era_id: EraId,
    auction_delay: u64,
    last_activation_point: EraId,
) -> Vec<BlockHeader>
where
    REv: From<StorageRequest>,
{
    let mut switch_blocks = Vec::new();
    let from = last_activation_point.max(era_id.saturating_sub(auction_delay).saturating_sub(1));
    for switch_block_era_id in (from.value()..era_id.value()).map(EraId::from) {
        match effect_builder
            .get_switch_block_header_at_era_id_from_storage(switch_block_era_id)
            .await
        {
            Some(switch_block) => switch_blocks.push(switch_block),
            None => {
                error!(
                    ?era_id,
                    ?switch_block_era_id,
                    "switch block header era must exist to initialize era"
                );
                panic!("switch block header not found in storage");
            }
        }
    }
    switch_blocks
}

async fn get_deploys_or_transfers<REv>(
    effect_builder: EffectBuilder<REv>,
    hashes: Vec<DeployHash>,
) -> Option<Vec<Deploy>>
where
    REv: From<StorageRequest>,
{
    let mut deploys_or_transfer: Vec<Deploy> = Vec::with_capacity(hashes.len());
    for maybe_deploy_or_transfer in effect_builder.get_deploys_from_storage(hashes).await {
        if let Some(deploy_or_transfer) = maybe_deploy_or_transfer {
            deploys_or_transfer.push(deploy_or_transfer)
        } else {
            return None;
        }
    }
    Some(deploys_or_transfer)
}

async fn execute_finalized_block<REv>(
    effect_builder: EffectBuilder<REv>,
    finalized_block: FinalizedBlock,
) where
    REv: From<StorageRequest> + From<ControlAnnouncement> + From<ContractRuntimeRequest>,
{
    // Get all deploys in order they appear in the finalized block.
    let deploys =
        match get_deploys_or_transfers(effect_builder, finalized_block.deploy_hashes().to_owned())
            .await
        {
            Some(deploys) => deploys,
            None => {
                fatal!(
                    effect_builder,
                    "Could not fetch deploys for finalized block: {:?}",
                    finalized_block
                )
                .await;
                return;
            }
        };

    // Get all transfers in order they appear in the finalized block.
    let transfers = match get_deploys_or_transfers(
        effect_builder,
        finalized_block.transfer_hashes().to_owned(),
    )
    .await
    {
        Some(transfers) => transfers,
        None => {
            fatal!(
                effect_builder,
                "Could not fetch transfers for finalized block: {:?}",
                finalized_block
            )
            .await;
            return;
        }
    };

    effect_builder
        .enqueue_block_for_execution(finalized_block, deploys, transfers)
        .await
}

/// Computes the instance ID for an era, given the era ID and the chainspec hash.
fn instance_id(protocol_config: &ProtocolConfig, era_id: EraId) -> Digest {
    let mut result = [0; Digest::LENGTH];
    let mut hasher = VarBlake2b::new(Digest::LENGTH).expect("should create hasher");

    hasher.update(protocol_config.chainspec_hash.as_ref());
    hasher.update(era_id.to_le_bytes());

    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    result.into()
}

/// Checks that a [BlockPayload] does not have deploys we have already included in blocks in
/// previous eras. This is done by repeatedly querying storage for deploy metadata. When metadata is
/// found storage is queried again to get the era id for the included deploy. That era id must *not*
/// be less than the current era, otherwise the deploy is a replay attack.
async fn check_deploys_for_replay_in_previous_eras_and_validate_block<REv, I>(
    effect_builder: EffectBuilder<REv>,
    proposed_block_era_id: EraId,
    sender: I,
    proposed_block: ProposedBlock<ClContext>,
) -> Event<I>
where
    REv: From<BlockValidationRequest<I>> + From<StorageRequest>,
    I: Clone + Send + 'static,
{
    for deploy_hash in proposed_block.value().deploys_and_transfers_iter() {
        let block_header = match effect_builder
            .get_block_header_for_deploy_from_storage(deploy_hash.into())
            .await
        {
            None => continue,
            Some(header) => header,
        };
        // We have found the deploy in the database. If it was from a previous era, it was a
        // replay attack.
        //
        // If not, then it might be this is a deploy for a block we are currently
        // coming to consensus, and we will rely on the immediate ancestors of the
        // block_payload within the current era to determine if we are facing a replay
        // attack.
        if block_header.era_id() < proposed_block_era_id {
            return Event::ResolveValidity(ResolveValidity {
                era_id: proposed_block_era_id,
                sender: sender.clone(),
                proposed_block: proposed_block.clone(),
                valid: false,
            });
        }
    }

    let sender_for_validate_block: I = sender.clone();
    let valid = effect_builder
        .validate_block(sender_for_validate_block, proposed_block.clone())
        .await;

    Event::ResolveValidity(ResolveValidity {
        era_id: proposed_block_era_id,
        sender,
        proposed_block,
        valid,
    })
}

impl ProposedBlock<ClContext> {
    /// If this block contains a deploy that's also present in an ancestor, this returns the deploy
    /// hash, otherwise `None`.
    fn contains_replay(&self) -> Option<DeployHash> {
        let block_deploys_set: BTreeSet<DeployOrTransferHash> =
            self.value().deploys_and_transfers_iter().collect();
        self.context()
            .ancestor_values()
            .iter()
            .flat_map(|ancestor| ancestor.deploys_and_transfers_iter())
            .find(|deploy| block_deploys_set.contains(deploy))
            .map(DeployOrTransferHash::into)
    }
}
