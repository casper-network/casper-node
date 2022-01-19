//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

pub(super) mod debug;
mod era;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::Error;
use datasize::DataSize;
use futures::FutureExt;
use itertools::Itertools;
use prometheus::Registry;
use rand::Rng;
use tracing::{debug, error, info, trace, warn};

use casper_hashing::Digest;
use casper_types::{AsymmetricType, EraId, PublicKey, SecretKey, U512};

pub use self::era::Era;
use crate::{
    components::consensus::{
        cl_context::{ClContext, Keypair},
        config::ProtocolConfig,
        consensus_protocol::{
            ConsensusProtocol, EraReport, FinalizedBlock as CpFinalizedBlock, ProposedBlock,
            ProtocolOutcome,
        },
        metrics::Metrics,
        traits::NodeIdT,
        validator_change::ValidatorChanges,
        ActionId, Config, ConsensusMessage, Event, NewBlockPayload, ReactorEventT, ResolveValidity,
        TimerId, ValidatorChange,
    },
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
    /// A map of consensus protocol instances.
    /// A value is a trait so that we can run different consensus protocols per era.
    ///
    /// This map always contains exactly `2 * bonded_eras + 1` entries, with the last one being the
    /// current one.
    open_eras: HashMap<EraId, Era<I>>,
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
    metrics: Metrics,
    /// The path to the folder where unit files will be stored.
    unit_files_folder: PathBuf,
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
        let ae: Vec<_> = self.open_eras.keys().collect();
        write!(formatter, "EraSupervisor {{ open_eras: {:?}, .. }}", ae)
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
        storage_dir: &Path,
        config: WithDir<Config>,
        effect_builder: EffectBuilder<REv>,
        protocol_config: ProtocolConfig,
        maybe_latest_block_header: Option<&BlockHeader>,
        next_upgrade_activation_point: Option<ActivationPoint>,
        registry: &Registry,
        new_consensus: Box<ConsensusConstructor<I>>,
    ) -> Result<(Self, Effects<Event<I>>), Error> {
        if current_era < protocol_config.last_activation_point {
            panic!(
                "Current era ({:?}) is before the last activation point ({:?}) - no eras would \
                be instantiated!",
                current_era, protocol_config.last_activation_point
            );
        }
        let unit_files_folder = storage_dir.join("unit_files");
        let (root, config) = config.into_parts();
        let (secret_signing_key, public_signing_key) = config.load_keys(root)?;
        info!(our_id = %public_signing_key, "EraSupervisor pubkey",);
        let metrics =
            Metrics::new(registry).expect("failed to set up and register consensus metrics");
        let activation_era_id = protocol_config.last_activation_point;
        let auction_delay = protocol_config.auction_delay;
        #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
        let next_height = maybe_latest_block_header.map_or(0, |hdr| hdr.height() + 1);

        let era_supervisor = Self {
            open_eras: Default::default(),
            secret_signing_key,
            public_signing_key,
            current_era,
            protocol_config,
            config,
            new_consensus,
            next_block_height: next_height,
            metrics,
            unit_files_folder,
            next_upgrade_activation_point,
            stop_for_upgrade: false,
            next_executed_height: next_height,
            era_where_we_joined: current_era,
        };

        let bonded_eras = era_supervisor.bonded_eras();
        let era_ids: Vec<EraId> = era_supervisor
            .iter_past(current_era, era_supervisor.bonded_eras().saturating_mul(3))
            .collect();

        // Asynchronously collect the information needed to initialize all recent eras.
        let effects = async move {
            info!(?era_ids, "collecting key blocks and booking blocks");

            let key_blocks = effect_builder
                .collect_key_block_headers(era_ids.iter().cloned())
                .await
                .expect("should have all the key blocks in storage");

            let booking_blocks = collect_booking_block_hashes(
                effect_builder,
                era_ids.clone(),
                auction_delay,
                activation_era_id,
            )
            .await;

            if current_era > activation_era_id.saturating_add(bonded_eras.saturating_mul(2).into())
            {
                // All eras can be initialized using the key blocks only.
                (key_blocks, booking_blocks, Default::default())
            } else {
                let activation_era_validators = effect_builder
                    .get_era_validators(activation_era_id)
                    .await
                    .unwrap_or_default();
                (key_blocks, booking_blocks, activation_era_validators)
            }
        }
        .event(
            move |(key_blocks, booking_blocks, validators)| Event::InitializeEras {
                key_blocks,
                booking_blocks,
                validators,
            },
        );

        Ok((era_supervisor, effects))
    }

    /// Returns a list of status changes of active validators.
    pub(super) fn get_validator_changes(
        &self,
    ) -> BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>> {
        let mut result: BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>> = BTreeMap::new();
        for ((_, era0), (era_id, era1)) in self.open_eras.iter().tuple_windows() {
            for (pub_key, change) in ValidatorChanges::new(era0, era1).0 {
                result.entry(pub_key).or_default().push((*era_id, change));
            }
        }
        result
    }

    fn era_seed(booking_block_hash: BlockHash, key_block_seed: Digest) -> u64 {
        let result = Digest::hash_pair(booking_block_hash, key_block_seed).value();
        u64::from_le_bytes(result[0..std::mem::size_of::<u64>()].try_into().unwrap())
    }

    /// Returns an iterator over era IDs of `num_eras` past eras, plus the provided one.
    pub(crate) fn iter_past(&self, era_id: EraId, num_eras: u64) -> impl Iterator<Item = EraId> {
        (self
            .protocol_config
            .last_activation_point
            .max(era_id.saturating_sub(num_eras))
            .value()..=era_id.value())
            .map(EraId::from)
    }

    /// Returns an iterator over era IDs of `num_eras` past eras, excluding the provided one.
    pub(crate) fn iter_past_other(
        &self,
        era_id: EraId,
        num_eras: u64,
    ) -> impl Iterator<Item = EraId> {
        (self
            .protocol_config
            .last_activation_point
            .max(era_id.saturating_sub(num_eras))
            .value()..era_id.value())
            .map(EraId::from)
    }

    /// Returns an iterator over era IDs of `num_eras` future eras, plus the provided one.
    fn iter_future(&self, era_id: EraId, num_eras: u64) -> impl Iterator<Item = EraId> {
        (era_id.value()..=era_id.value().saturating_add(num_eras)).map(EraId::from)
    }

    /// Starts a new era; panics if it already exists.
    #[allow(clippy::too_many_arguments)] // FIXME
    fn new_era(
        &mut self,
        era_id: EraId,
        now: Timestamp,
        validators: BTreeMap<PublicKey, U512>,
        new_faulty: Vec<PublicKey>,
        faulty: HashSet<PublicKey>,
        inactive: HashSet<PublicKey>,
        seed: u64,
        start_time: Timestamp,
        start_height: u64,
    ) -> Vec<ProtocolOutcome<I, ClContext>> {
        if self.open_eras.contains_key(&era_id) {
            panic!("{} already exists", era_id);
        }
        let instance_id = instance_id(&self.protocol_config, era_id);

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

        // Activate the era if this node was already running when the era began, it is still
        // ongoing based on its minimum duration, and we are one of the validators.
        let our_id = &self.public_signing_key;
        let should_activate = if self.current_era > era_id {
            trace!(
                era = era_id.value(),
                current_era = self.current_era.value(),
                "not voting; initializing past era"
            );
            false
        } else if !validators.contains_key(our_id) {
            info!(era = era_id.value(), %our_id, "not voting; not a validator");
            false
        } else {
            info!(era = era_id.value(), %our_id, "start voting");
            true
        };
        if era_id >= self.current_era {
            self.current_era = era_id;
            self.metrics.current_era.set(era_id.value() as i64);
        }

        let prev_era = era_id
            .checked_sub(1)
            .and_then(|last_era_id| self.open_eras.get(&last_era_id));

        let (mut consensus, mut outcomes) = (self.new_consensus)(
            instance_id,
            validators.clone(),
            &faulty,
            &inactive,
            &self.protocol_config,
            &self.config,
            prev_era.map(|era| &*era.consensus),
            start_time,
            seed,
            now,
        );

        if should_activate {
            let secret = Keypair::new(self.secret_signing_key.clone(), our_id.clone());
            outcomes.extend(consensus.activate_validator(
                our_id.clone(),
                secret,
                now,
                Some(self.unit_file(&instance_id)),
            ))
        }

        let mut era = Era::new(
            consensus,
            start_time,
            start_height,
            new_faulty,
            faulty,
            inactive,
            validators,
        );

        // Mark validators as faulty for which we have evidence in a recent era.
        for e_id in self.iter_past_other(era_id, self.bonded_eras()) {
            if let Some(old_era) = self.open_eras.get_mut(&e_id) {
                for pub_key in old_era.consensus.validators_with_evidence() {
                    let proposed_blocks = era.resolve_evidence_and_mark_faulty(pub_key);
                    if !proposed_blocks.is_empty() {
                        error!(
                            ?proposed_blocks,
                            era = e_id.value(),
                            "unexpected block in new era"
                        );
                    }
                }
            }
        }

        let _ = self.open_eras.insert(era_id, era);
        let oldest_bonded_era_id = oldest_bonded_era(&self.protocol_config, era_id);
        // Clear the obsolete data from the era whose validators are unbonded now. We only retain
        // the information necessary to validate evidence that units in still-bonded eras may refer
        // to for cross-era fault tracking.
        if let Some(evidence_only_era_id) = oldest_bonded_era_id.checked_sub(1) {
            trace!(era = evidence_only_era_id.value(), "clearing unbonded era");
            if let Some(era) = self.open_eras.get_mut(&evidence_only_era_id) {
                era.consensus.set_evidence_only();
            }
        }
        // Remove the era that has become obsolete now: The oldest bonded era could still receive
        // units that refer to evidence from any era that was bonded when it was the current one.
        let oldest_evidence_era_id = oldest_bonded_era(&self.protocol_config, oldest_bonded_era_id);
        if let Some(obsolete_era_id) = oldest_evidence_era_id.checked_sub(1) {
            if let Some(_era) = self.open_eras.remove(&obsolete_era_id) {
                trace!(era = obsolete_era_id.value(), "removing obsolete era");
            }
        }

        outcomes
    }

    fn prune_unit_files(&self) {
        let valid_unit_files: HashSet<_> = self
            .open_eras
            .iter()
            .map(|(_, era)| self.unit_file(era.consensus.instance_id()))
            .collect();

        let dir_iterator = match fs::read_dir(&self.unit_files_folder) {
            Ok(iter) => iter,
            Err(err) => {
                warn!(?err, path=?self.unit_files_folder, "could not read the unit files folder");
                // if we couldn't clean up the unit files, we just return
                return;
            }
        };

        for entry in dir_iterator {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(
                        ?err,
                        path=?self.unit_files_folder,
                        "error while reading the unit files folder",
                    );
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                // unit files should be stored directly in the folder, not in subdirectories
                continue;
            }
            if valid_unit_files.contains(&path) {
                // don't remove files corresponding to active eras
                continue;
            }
            debug!(?path, "removing unit file");
            if let Err(err) = fs::remove_file(&path) {
                warn!(?err, ?path, "could not delete unit file");
            }
        }
    }

    /// Returns `true` if the specified era is open and bonded.
    fn is_bonded(&self, era_id: EraId) -> bool {
        era_id.saturating_add(self.bonded_eras().into()) >= self.current_era
            && era_id <= self.current_era
    }

    /// Returns whether the validator with the given public key is bonded in that era.
    fn is_validator_in(&self, pub_key: &PublicKey, era_id: EraId) -> bool {
        let has_validator = |era: &Era<I>| era.validators().contains_key(pub_key);
        self.open_eras.get(&era_id).map_or(false, has_validator)
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
        match self.open_eras.get_mut(&self.current_era) {
            Some(era) => era.set_paused(paused),
            None => error!(
                era = self.current_era.value(),
                "current era not initialized"
            ),
        }
    }

    pub(super) fn handle_initialize_eras<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        key_blocks: HashMap<EraId, BlockHeader>,
        booking_blocks: HashMap<EraId, BlockHash>,
        activation_era_validators: BTreeMap<PublicKey, U512>,
    ) -> Effects<Event<I>> {
        let mut effects = Effects::new();
        let now = Timestamp::now();

        for era_id in self.iter_past(self.current_era, self.bonded_eras().saturating_mul(2)) {
            let new_faulty;
            let validators;
            let start_height;
            let era_start_time;
            let seed;

            let booking_block_hash = booking_blocks
                .get(&era_id)
                .expect("should have booking block");

            #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
            if era_id.is_genesis() {
                new_faulty = vec![];
                // The validator set was read from the global state: there's no key block for era 0.
                validators = activation_era_validators.clone();
                start_height = 0;
                era_start_time = self
                    .protocol_config
                    .genesis_timestamp
                    .expect("must have genesis start time if era ID is 0");
                seed = 0;
            } else {
                // If this is not era 0, there must be a key block for it.
                let key_block = key_blocks.get(&era_id).expect("missing key block");
                start_height = key_block.height() + 1;
                era_start_time = key_block.timestamp();
                seed = Self::era_seed(*booking_block_hash, key_block.accumulated_seed());
                if era_id == self.protocol_config.last_activation_point {
                    // After an upgrade or emergency restart, we don't track faults cross-era.
                    new_faulty = vec![];
                    // And we read the validator sets from the global state, because the key block
                    // might have been overwritten by the upgrade/restart.
                    validators = activation_era_validators.clone();
                } else {
                    // If it's neither genesis nor upgrade nor restart, we use the validators from
                    // the key block and ban validators that were faulty in previous eras.
                    new_faulty = key_block
                        .era_end()
                        .expect("key block must be a switch block")
                        .era_report()
                        .equivocators
                        .clone();
                    validators = key_block
                        .next_era_validator_weights()
                        .expect("missing validators from key block")
                        .clone();
                }
            }

            let faulty = self
                .iter_past(era_id, self.banning_period())
                .filter_map(|old_id| key_blocks.get(&old_id).and_then(|bhdr| bhdr.era_end()))
                .flat_map(|era_end| era_end.era_report().equivocators.clone())
                .collect();

            let results = self.new_era(
                era_id,
                now,
                validators,
                new_faulty,
                faulty,
                key_blocks
                    .get(&era_id)
                    .and_then(|bhdr| bhdr.era_end())
                    .into_iter()
                    .flat_map(|era_end| &era_end.era_report().inactive_validators)
                    .cloned()
                    .collect(),
                seed,
                era_start_time,
                start_height,
            );
            effects.extend(self.handle_consensus_outcomes(effect_builder, rng, era_id, results));
        }

        self.prune_unit_files();

        let open_era_outcomes = self.open_eras[&self.current_era]
            .consensus
            .handle_is_current(now);
        self.next_block_height = self.open_eras[&self.current_era].start_height;
        effects.extend(self.handle_consensus_outcomes(
            effect_builder,
            rng,
            self.current_era,
            open_era_outcomes,
        ));

        info!("finished initializing era supervisor");
        info!(?self, "current eras");

        effects
    }

    /// The number of past eras whose validators are still bonded. After this many eras, a former
    /// validator is allowed to withdraw their stake, so their signature can't be trusted anymore.
    ///
    /// A node keeps `2 * bonded_eras` past eras around, because the oldest bonded era could still
    /// receive blocks that refer to `bonded_eras` before that.
    fn bonded_eras(&self) -> u64 {
        bonded_eras(&self.protocol_config)
    }

    /// The number of past eras we have to check for faulty validators that will be banned in the
    /// next era.
    // TODO: This should just be `auction_delay`, but we need to guarantee we have enough
    // eras.
    fn banning_period(&self) -> u64 {
        self.bonded_eras().min(self.protocol_config.auction_delay)
    }

    /// Returns the path to the era's unit file.
    fn unit_file(&self, instance_id: &Digest) -> PathBuf {
        self.unit_files_folder.join(format!(
            "unit_{:?}_{}.dat",
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
        F: FnOnce(
            &mut dyn ConsensusProtocol<I, ClContext>,
            &mut NodeRng,
        ) -> Vec<ProtocolOutcome<I, ClContext>>,
    {
        match self.open_eras.get_mut(&era_id) {
            None => {
                if era_id > self.current_era {
                    info!(era = era_id.value(), "received message for future era");
                } else {
                    info!(era = era_id.value(), "received message for obsolete era");
                }
                Effects::new()
            }
            Some(era) => {
                let outcomes = f(&mut *era.consensus, rng);
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
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, _| {
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
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, _| {
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
                self.delegate_to_era(effect_builder, rng, era_id, move |consensus, rng| {
                    consensus.handle_message(rng, sender, payload, Timestamp::now())
                })
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => {
                if !self.is_bonded(era_id) {
                    trace!(era = era_id.value(), "not handling message; era too old");
                    return Effects::new();
                }
                self.iter_past(era_id, self.bonded_eras())
                    .flat_map(|e_id| {
                        self.delegate_to_era(effect_builder, rng, e_id, |consensus, _| {
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
        if !self.is_bonded(era_id) {
            warn!(era = era_id.value(), "new block payload in outdated era");
            return Effects::new();
        }
        let proposed_block = ProposedBlock::new(block_payload, block_context);
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, _| {
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
        if block_header.is_switch_block() {
            if let Some(era) = self.open_eras.get_mut(&era_id) {
                // This was the era's last block. Schedule deactivating this era.
                let delay = Timestamp::now()
                    .saturating_diff(block_header.timestamp())
                    .into();
                let faulty_num = era.consensus.validators_with_evidence().len();
                let deactivate_era = move |_| Event::DeactivateEra {
                    era_id,
                    faulty_num,
                    delay,
                };
                effects.extend(effect_builder.set_timeout(delay).event(deactivate_era));
            } else {
                error!(era = era_id.value(), %block_header, "executed block in uninitialized era");
            }
            // If it's not the last block before an upgrade, initialize the next era.
            if !self.should_upgrade_after(&era_id) {
                let new_era_id = era_id.successor();
                let effect = get_booking_block_hash(
                    effect_builder,
                    new_era_id,
                    self.protocol_config.auction_delay,
                    self.protocol_config.last_activation_point,
                )
                .event(move |booking_block_hash| Event::CreateNewEra {
                    switch_block_header: Box::new(block_header),
                    booking_block_hash: Ok(booking_block_hash),
                });
                effects.extend(effect);
            }
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
        let era = if let Some(era) = self.open_eras.get_mut(&era_id) {
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

    /// Creates a new era.
    pub(super) fn handle_create_new_era<REv: ReactorEventT<I>>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        switch_block_header: BlockHeader,
        booking_block_hash: BlockHash,
    ) -> Effects<Event<I>> {
        let (era_report, next_era_validators_weights) = match (
            switch_block_header.era_end(),
            switch_block_header.next_era_validator_weights(),
        ) {
            (Some(era_end), Some(next_era_validator_weights)) => {
                (era_end.era_report(), next_era_validator_weights)
            }
            _ => {
                return fatal!(
                    effect_builder,
                    "attempted to create a new era with a non-switch block: {}",
                    switch_block_header
                )
                .ignore()
            }
        };
        let new_faulty = era_report.equivocators.clone();
        let era_id = switch_block_header.era_id().successor();
        info!(era = era_id.value(), "era created");
        let seed = EraSupervisor::<I>::era_seed(
            booking_block_hash,
            switch_block_header.accumulated_seed(),
        );
        trace!(%seed, "the seed for {}: {}", era_id, seed);
        let faulty = self
            .iter_past_other(era_id, self.banning_period())
            .flat_map(|e_id| &self.open_eras[&e_id].new_faulty)
            .chain(&new_faulty)
            .cloned()
            .collect();
        let now = Timestamp::now(); // TODO: This should be passed in.
        #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
        let mut outcomes = self.new_era(
            era_id,
            now,
            next_era_validators_weights.clone(),
            new_faulty,
            faulty,
            era_report.inactive_validators.iter().cloned().collect(),
            seed,
            switch_block_header.timestamp(),
            switch_block_header.height() + 1,
        );
        self.prune_unit_files();
        outcomes.extend(self.open_eras[&era_id].consensus.handle_is_current(now));
        self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
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
                peer_id = %sender,
                era = %era_id.value(),
                "invalid consensus value; disconnecting from the sender"
            );
            effects.extend(self.disconnect(effect_builder, sender));
        }
        if self
            .open_eras
            .get_mut(&era_id)
            .map_or(false, |era| era.resolve_validity(&proposed_block, valid))
        {
            effects.extend(
                self.delegate_to_era(effect_builder, rng, era_id, |consensus, _| {
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
        self.iter_past(era_id, self.bonded_eras())
            .any(|eid| self.era(eid).consensus.has_evidence(&pub_key))
    }

    /// Returns the era with the specified ID. Panics if it does not exist.
    fn era(&self, era_id: EraId) -> &Era<I> {
        &self.open_eras[&era_id]
    }

    /// Returns the era with the specified ID mutably. Panics if it does not exist.
    fn era_mut(&mut self, era_id: EraId) -> &mut Era<I> {
        self.open_eras.get_mut(&era_id).unwrap()
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
            ProtocolOutcome::CreatedMessageToRandomPeer(payload) => {
                let message = ConsensusMessage::Protocol { era_id, payload };

                async move {
                    let peers = effect_builder.get_fully_connected_peers().await;
                    if let Some(to) = peers.into_iter().next() {
                        effect_builder.send_message(to, message.into()).await;
                    }
                }
                .ignore()
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
                    .iter_past(era_id, self.bonded_eras())
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
                let era = self.open_eras.get_mut(&era_id).unwrap();
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
                info!(
                    era_id = ?finalized_block.era_id(),
                    height = ?finalized_block.height(),
                    timestamp = ?finalized_block.timestamp(),
                    "finalized block"
                );
                self.metrics.finalized_block(&finalized_block);
                // Announce the finalized block.
                let mut effects = effect_builder
                    .announce_finalized_block(finalized_block.clone())
                    .ignore();
                self.next_block_height = self.next_block_height.max(finalized_block.height() + 1);
                // Request execution of the finalized block.
                effects.extend(execute_finalized_block(effect_builder, finalized_block).ignore());
                self.update_consensus_pause();
                effects
            }
            ProtocolOutcome::ValidateConsensusValue {
                sender,
                proposed_block,
            } => {
                if !self.is_bonded(era_id) {
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
                for e_id in self.iter_future(era_id, self.bonded_eras()) {
                    let proposed_blocks = if let Some(era) = self.open_eras.get_mut(&e_id) {
                        era.resolve_evidence_and_mark_faulty(&pub_key)
                    } else {
                        continue;
                    };
                    for proposed_block in proposed_blocks {
                        effects.extend(self.delegate_to_era(
                            effect_builder,
                            rng,
                            e_id,
                            |consensus, _| {
                                consensus.resolve_validity(proposed_block, true, Timestamp::now())
                            },
                        ));
                    }
                }
                effects
            }
            ProtocolOutcome::SendEvidence(sender, pub_key) => self
                .iter_past_other(era_id, self.bonded_eras())
                .flat_map(|e_id| {
                    self.delegate_to_era(effect_builder, rng, e_id, |consensus, _| {
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
            .open_eras
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

    /// Returns the most recent era.
    pub(crate) fn current_era(&self) -> EraId {
        self.current_era
    }

    /// Get a reference to the era supervisor's open eras.
    pub(crate) fn open_eras(&self) -> &HashMap<EraId, Era<I>> {
        &self.open_eras
    }
}

#[cfg(test)]
impl<I> EraSupervisor<I>
where
    I: NodeIdT,
{
    /// Returns the list of validators who equivocated in this era.
    pub(crate) fn validators_with_evidence(&self, era_id: EraId) -> Vec<&PublicKey> {
        self.open_eras[&era_id].consensus.validators_with_evidence()
    }

    /// Returns this node's validator key.
    pub(crate) fn public_key(&self) -> &PublicKey {
        &self.public_signing_key
    }
}

/// Returns an era ID in which the booking block for `era_id` lives, if we can use it.
/// Booking block for era N is the switch block (the last block) in era N – AUCTION_DELAY - 1.
/// To find it, we get the start height of era N - AUCTION_DELAY and subtract 1.
/// We make sure not to use an era ID below the last upgrade activation point, because we will
/// not have instances of eras from before that.
///
/// We can't use it if it is:
/// * before Genesis
/// * before upgrade
/// * before emergency restart
/// In those cases, returns `None`.
fn valid_booking_block_era_id(
    era_id: EraId,
    auction_delay: u64,
    last_activation_point: EraId,
) -> Option<EraId> {
    let after_booking_era_id = era_id.saturating_sub(auction_delay);

    // If we would have gone below the last activation point (the first `AUCTION_DELAY ` eras after
    // an upgrade), we return `None` as there are no booking blocks there that we can use – we
    // can't use anything from before an upgrade.
    // NOTE that it's OK if `booking_era_id` == `last_activation_point`.
    (after_booking_era_id > last_activation_point).then(|| after_booking_era_id.saturating_sub(1))
}

/// Returns a booking block hash for `era_id`.
async fn get_booking_block_hash<REv>(
    effect_builder: EffectBuilder<REv>,
    era_id: EraId,
    auction_delay: u64,
    last_activation_point: EraId,
) -> BlockHash
where
    REv: From<StorageRequest>,
{
    if let Some(booking_block_era_id) =
        valid_booking_block_era_id(era_id, auction_delay, last_activation_point)
    {
        match effect_builder
            .get_switch_block_header_at_era_id_from_storage(booking_block_era_id)
            .await
        {
            Some(block_header) => block_header.hash(),
            None => {
                error!(
                    ?era_id,
                    ?booking_block_era_id,
                    "booking block header for era must exist"
                );
                panic!("booking block header not found in storage");
            }
        }
    } else {
        // If there's no booking block for the `era_id`
        // (b/c it would have been from before Genesis, upgrade or emergency restart),
        // use a "zero" block hash. This should not hurt the security of the leader selection
        // algorithm.
        BlockHash::default()
    }
}

/// Returns booking block hashes for the eras.
async fn collect_booking_block_hashes<REv>(
    effect_builder: EffectBuilder<REv>,
    era_ids: Vec<EraId>,
    auction_delay: u64,
    last_activation_point: EraId,
) -> HashMap<EraId, BlockHash>
where
    REv: From<StorageRequest>,
{
    let mut booking_block_hashes: HashMap<EraId, BlockHash> = HashMap::new();

    for era_id in era_ids {
        let booking_block_hash =
            get_booking_block_hash(effect_builder, era_id, auction_delay, last_activation_point)
                .await;
        booking_block_hashes.insert(era_id, booking_block_hash);
    }

    booking_block_hashes
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
    Digest::hash_pair(protocol_config.chainspec_hash, era_id.to_le_bytes())
        .value()
        .into()
}

/// The number of past eras whose validators are still bonded. After this many eras, a former
/// validator is allowed to withdraw their stake, so their signature can't be trusted anymore.
///
/// A node keeps `2 * bonded_eras` past eras around, because the oldest bonded era could still
/// receive blocks that refer to `bonded_eras` before that.
fn bonded_eras(protocol_config: &ProtocolConfig) -> u64 {
    protocol_config
        .unbonding_delay
        .saturating_sub(protocol_config.auction_delay)
}

/// The oldest era whose validators are still bonded.
// This is public because it's used in reactor::validator::tests.
pub(crate) fn oldest_bonded_era(protocol_config: &ProtocolConfig, current_era: EraId) -> EraId {
    current_era
        .saturating_sub(bonded_eras(protocol_config))
        .max(protocol_config.last_activation_point)
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
