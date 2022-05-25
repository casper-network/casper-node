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
    fs, io,
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
use casper_types::{AsymmetricType, EraId, PublicKey, SecretKey, TimeDiff, Timestamp, U512};

use crate::{
    components::{
        consensus::{
            cl_context::{ClContext, Keypair},
            consensus_protocol::{
                ConsensusProtocol, EraReport, FinalizedBlock as CpFinalizedBlock, ProposedBlock,
                ProtocolOutcome,
            },
            metrics::Metrics,
            validator_change::{ValidatorChange, ValidatorChanges},
            ActionId, ChainspecConsensusExt, Config, ConsensusMessage, Event, NewBlockPayload,
            ReactorEventT, ResolveValidity, TimerId,
        },
        storage::Storage,
    },
    effect::{
        announcements::ControlAnnouncement,
        requests::{BlockValidationRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::{
        ActivationPoint, BlockHash, BlockHeader, Chainspec, Deploy, DeployHash,
        DeployOrTransferHash, FinalitySignature, FinalizedApprovals, FinalizedBlock, NodeId,
    },
    NodeRng,
};

pub use self::era::Era;
use crate::components::consensus::error::CreateNewEraError;

/// The delay in milliseconds before we shutdown after the number of faulty validators exceeded the
/// fault tolerance threshold.
const FTT_EXCEEDED_SHUTDOWN_DELAY_MILLIS: u64 = 60 * 1000;

/// The number of eras across which evidence can be cited.
/// If this is 1, you can cite evidence from the previous era, but not the one before that.
/// To be able to detect that evidence, we also keep that number of active past eras in memory.
const PAST_EVIDENCE_ERAS: u64 = 1;
/// The total number of past eras that are kept in memory in addition to the current one.
/// The more recent half of these is active: it contains units and can still accept further units.
/// The older half is in evidence-only state, and only used to validate cited evidence.
pub(super) const PAST_OPEN_ERAS: u64 = 2 * PAST_EVIDENCE_ERAS;

type ConsensusConstructor = dyn Fn(
        Digest,                    // the era's unique instance ID
        BTreeMap<PublicKey, U512>, // validator weights
        &HashSet<PublicKey>,       /* faulty validators that are banned in
                                    * this era */
        &HashSet<PublicKey>, // inactive validators that can't be leaders
        &Chainspec,          // the network's chainspec
        &Config,             // The consensus part of the node config.
        Option<&dyn ConsensusProtocol<ClContext>>, // previous era's consensus instance
        Timestamp,           // start time for this era
        u64,                 // random seed
        Timestamp,           // now timestamp
    ) -> (
        Box<dyn ConsensusProtocol<ClContext>>,
        Vec<ProtocolOutcome<ClContext>>,
    ) + Send;

#[derive(DataSize)]
pub struct EraSupervisor {
    /// A map of consensus protocol instances.
    /// A value is a trait so that we can run different consensus protocols per era.
    ///
    /// This map contains three consecutive entries, with the last one being the current era N. Era
    /// N - 1 is also kept in memory so that we would still detect any equivocations there and use
    /// them in era N to get the equivocator banned. And era N - 2 one is in an "evidence-only"
    /// state: It doesn't accept any new Highway units anymore, but we keep the instance in memory
    /// so we can evaluate evidence that units in era N - 1 might cite.
    ///
    /// Since eras at or before the most recent activation point are never instantiated, shortly
    /// after that there can temporarily be fewer than three entries in the map.
    open_eras: HashMap<EraId, Era>,
    secret_signing_key: Arc<SecretKey>,
    public_signing_key: PublicKey,
    current_era: EraId,
    chainspec: Arc<Chainspec>,
    config: Config,
    #[data_size(skip)] // Negligible for most closures, zero for functions.
    new_consensus: Box<ConsensusConstructor>,
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
    /// The era that was current when this node joined the network.
    era_where_we_joined: EraId,
}

impl Debug for EraSupervisor {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        let ae: Vec<_> = self.open_eras.keys().collect();
        write!(formatter, "EraSupervisor {{ open_eras: {:?}, .. }}", ae)
    }
}

impl EraSupervisor {
    /// Creates a new `EraSupervisor`, starting in the indicated current era.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new<REv: ReactorEventT>(
        current_era: EraId,
        storage_dir: &Path,
        secret_signing_key: Arc<SecretKey>,
        public_signing_key: PublicKey,
        config: Config,
        effect_builder: EffectBuilder<REv>,
        chainspec: Arc<Chainspec>,
        latest_block_header: &BlockHeader,
        next_upgrade_activation_point: Option<ActivationPoint>,
        registry: &Registry,
        new_consensus: Box<ConsensusConstructor>,
        storage: &Storage,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Event>), Error> {
        if current_era <= chainspec.activation_era() {
            panic!(
                "Current era ({:?}) is before the last activation point ({:?}) - no eras would \
                be instantiated!",
                current_era,
                chainspec.activation_era()
            );
        }
        let unit_files_folder = storage_dir.join("unit_files");
        info!(our_id = %public_signing_key, "EraSupervisor pubkey",);
        let metrics =
            Metrics::new(registry).expect("failed to set up and register consensus metrics");
        #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
        let next_height = latest_block_header.height() + 1;

        let mut era_supervisor = Self {
            open_eras: Default::default(),
            secret_signing_key,
            public_signing_key,
            current_era,
            chainspec,
            config,
            new_consensus,
            next_block_height: next_height,
            metrics,
            unit_files_folder,
            next_upgrade_activation_point,
            next_executed_height: next_height,
            era_where_we_joined: current_era,
        };

        // Collect the information needed to initialize all open eras.
        //
        // We need to initialize current_era, current_era - 1 and (evidence-only) current_era - 2.
        // To initialize an era, all switch blocks between its booking block and its key block are
        // required. The booking block for era N is in N - auction_delay - 1, and the key block in
        // N - 1. So we need all switch blocks between:
        // (including) current_era - 2 - auction_delay - 1 and (excluding) current_era.
        // However, we never use any block from before the last activation point.
        //
        // Example: If auction_delay is 1, to initialize era N we need the switch blocks from era N
        // and N - 1. If current_era is 10, we will initialize eras 10, 9 and 8. So we need the
        // switch blocks from eras 9, 8, 7 and 6.
        let earliest_open_era = era_supervisor.chainspec.earliest_open_era(current_era);
        let earliest_era = era_supervisor
            .chainspec
            .earliest_switch_block_needed(earliest_open_era);
        let mut switch_blocks = Vec::new();
        for era_id in (earliest_era.value()..current_era.value()).map(EraId::from) {
            let switch_block = storage
                .read_switch_block_header_by_era_id(era_id)?
                .ok_or_else(|| anyhow::Error::msg(format!("No such switch block in {}", era_id)))?;
            switch_blocks.push(switch_block);
        }

        // The create_new_era method initializes the era that the slice's last block is the key
        // block for. We want to initialize the three latest eras, so we have to pass in the whole
        // slice for the current era, and omit one or two elements for the other two. We never
        // initialize the activation era or an earlier era, however.
        //
        // In the example above, we would call create_new_era with the switch blocks from eras
        // 8 and 9 (to initialize 10) then 7 and 8 (for era 9), and finally 6 and 7 (for era 8).
        // (We don't truncate the slice at the start since unneeded blocks are ignored.)
        let mut effects = Effects::new();
        let from = switch_blocks
            .len()
            .saturating_sub(PAST_OPEN_ERAS as usize)
            .max(1);
        for i in (from..=switch_blocks.len()).rev() {
            effects.extend(era_supervisor.create_new_era_effects(
                effect_builder,
                rng,
                &switch_blocks[..i],
            ));
        }

        Ok((era_supervisor, effects))
    }

    /// Returns the merkle tree hash activation from the chainspec.
    fn verifiable_chunked_hash_activation(&self) -> EraId {
        self.chainspec
            .protocol_config
            .verifiable_chunked_hash_activation
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
    ///
    /// Note: Excludes the activation point era and earlier eras. The activation point era itself
    /// contains only the single switch block we created after the upgrade. There is no consensus
    /// instance for it.
    pub(crate) fn iter_past(&self, era_id: EraId, num_eras: u64) -> impl Iterator<Item = EraId> {
        (self
            .chainspec
            .activation_era()
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
            .chainspec
            .activation_era()
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
        let has_validator = |era: &Era| era.validators().contains_key(pub_key);
        self.open_eras.get(&era_id).map_or(false, has_validator)
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

    /// Initializes a new era. The switch blocks must contain the most recent `auction_delay + 1`
    /// ones, in order, but at most as far back as to the last activation point.
    pub(super) fn create_new_era_effects<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        switch_blocks: &[BlockHeader],
    ) -> Effects<Event> {
        match self.create_new_era(switch_blocks) {
            Ok((era_id, outcomes)) => {
                self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
            }
            Err(err) => fatal!(
                effect_builder,
                "failed to create era; this is a bug: {:?}",
                err,
            )
            .ignore(),
        }
    }

    /// Initializes a new era. The switch blocks must contain the most recent `auction_delay + 1`
    /// ones, in order, but at most as far back as to the last activation point.
    fn create_new_era(
        &mut self,
        switch_blocks: &[BlockHeader],
    ) -> Result<(EraId, Vec<ProtocolOutcome<ClContext>>), CreateNewEraError> {
        let key_block = switch_blocks
            .last()
            .ok_or(CreateNewEraError::AttemptedToCreateEraWithNoSwitchBlocks)?;
        let era_id = key_block.era_id().successor();
        let era_end = key_block.era_end().ok_or_else(|| {
            CreateNewEraError::LastBlockHeaderNotASwitchBlock {
                era_id,
                last_block_header: Box::new(key_block.clone()),
            }
        })?;

        let earliest_era = self.chainspec.earliest_switch_block_needed(era_id);
        let switch_blocks_needed = era_id.value().saturating_sub(earliest_era.value()) as usize;
        let first_idx = switch_blocks
            .len()
            .checked_sub(switch_blocks_needed)
            .ok_or_else(|| CreateNewEraError::InsufficientSwitchBlocks {
                era_id,
                switch_blocks: switch_blocks.to_vec(),
            })?;
        for (i, switch_block) in switch_blocks[first_idx..].iter().enumerate() {
            if switch_block.era_id() != earliest_era.saturating_add(i as u64) {
                return Err(CreateNewEraError::WrongSwitchBlockEra {
                    era_id,
                    switch_blocks: switch_blocks.to_vec(),
                });
            }
        }

        let report = era_end.era_report();
        let validators = era_end.next_era_validator_weights();

        if self.open_eras.contains_key(&era_id) {
            warn!(era = era_id.value(), "era already exists");
            return Ok((era_id, vec![]));
        }
        if self.current_era > era_id.saturating_add(PAST_OPEN_ERAS) {
            warn!(era = era_id.value(), "trying to create obsolete era");
            return Ok((era_id, vec![]));
        }

        // Compute the seed for the PRNG from the booking block hash and the accumulated seed.
        let auction_delay = self.chainspec.core_config.auction_delay as usize;
        let booking_block_hash =
            if let Some(booking_block) = switch_blocks.iter().rev().nth(auction_delay) {
                booking_block.hash(self.verifiable_chunked_hash_activation())
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
            .filter_map(|switch_block| switch_block.era_end())
            .flat_map(|era_end| era_end.era_report().equivocators.clone())
            .collect();

        let chainspec_hash = self.chainspec.hash();
        let key_block_hash = key_block.hash(self.verifiable_chunked_hash_activation());
        let instance_id = instance_id(chainspec_hash, era_id, key_block_hash);
        let now = Timestamp::now();

        info!(
            ?validators,
            %start_time,
            %now,
            %start_height,
            %chainspec_hash,
            %key_block_hash,
            %instance_id,
            %seed,
            era = era_id.value(),
            "starting era",
        );

        let maybe_prev_era = era_id
            .checked_sub(1)
            .and_then(|last_era_id| self.open_eras.get(&last_era_id));
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
            self.chainspec.as_ref(),
            &self.config,
            maybe_prev_era.map(|prev_era| &*prev_era.consensus),
            start_time,
            seed,
            now,
        );
        let era = Era::new(
            consensus,
            start_time,
            start_height,
            faulty,
            inactive,
            validators.clone(),
        );
        let _ = self.open_eras.insert(era_id, era);

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
            outcomes.extend(self.era_mut(era_id).consensus.handle_is_current(now));
            if !self.era(era_id).validators().contains_key(&our_id) {
                info!(era = era_id.value(), %our_id, "not voting; not a validator");
            } else {
                info!(era = era_id.value(), %our_id, "start voting");
                let secret = Keypair::new(self.secret_signing_key.clone(), our_id.clone());
                let unit_hash_file = self.unit_file(&instance_id);
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
        if let Some(evidence_only_era_id) = self.current_era.checked_sub(PAST_OPEN_ERAS) {
            if let Some(era) = self.open_eras.get_mut(&evidence_only_era_id) {
                trace!(era = evidence_only_era_id.value(), "clearing unbonded era");
                era.consensus.set_evidence_only();
            }

            // Remove the era that has become obsolete now: We keep only three in memory.
            if let Some(obsolete_era_id) = evidence_only_era_id.checked_sub(1) {
                if let Some(era) = self.open_eras.remove(&obsolete_era_id) {
                    trace!(era = obsolete_era_id.value(), "removing obsolete era");
                    match fs::remove_file(self.unit_file(era.consensus.instance_id())) {
                        Ok(_) => {}
                        Err(err) => match err.kind() {
                            io::ErrorKind::NotFound => {}
                            err => warn!(?err, "could not delete unit hash file"),
                        },
                    }
                }
            }
        }

        Ok((era_id, outcomes))
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
    fn delegate_to_era<REv: ReactorEventT, F>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        f: F,
    ) -> Effects<Event>
    where
        F: FnOnce(
            &mut dyn ConsensusProtocol<ClContext>,
            &mut NodeRng,
        ) -> Vec<ProtocolOutcome<ClContext>>,
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

    pub(super) fn handle_timer<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        timestamp: Timestamp,
        timer_id: TimerId,
    ) -> Effects<Event> {
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, _| {
            consensus.handle_timer(timestamp, timer_id)
        })
    }

    pub(super) fn handle_action<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        action_id: ActionId,
    ) -> Effects<Event> {
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, _| {
            consensus.handle_action(action_id, Timestamp::now())
        })
    }

    pub(super) fn handle_message<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        sender: NodeId,
        msg: ConsensusMessage,
    ) -> Effects<Event> {
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
                if era_id.saturating_add(PAST_EVIDENCE_ERAS) < self.current_era
                    || !self.open_eras.contains_key(&era_id)
                {
                    trace!(era = era_id.value(), "not handling message; era too old");
                    return Effects::new();
                }
                self.iter_past(era_id, PAST_EVIDENCE_ERAS)
                    .flat_map(|e_id| {
                        self.delegate_to_era(effect_builder, rng, e_id, |consensus, _| {
                            consensus.request_evidence(sender, &pub_key)
                        })
                    })
                    .collect()
            }
        }
    }

    pub(super) fn handle_new_block_payload<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        new_block_payload: NewBlockPayload,
    ) -> Effects<Event> {
        let NewBlockPayload {
            era_id,
            block_payload,
            block_context,
        } = new_block_payload;
        if era_id.saturating_add(PAST_EVIDENCE_ERAS) < self.current_era
            || !self.open_eras.contains_key(&era_id)
        {
            warn!(era = era_id.value(), "new block payload in outdated era");
            return Effects::new();
        }
        let proposed_block = ProposedBlock::new(block_payload, block_context);
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, _| {
            consensus.propose(proposed_block, Timestamp::now())
        })
    }

    pub(super) fn handle_block_added<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_header: BlockHeader,
    ) -> Effects<Event> {
        let our_pk = self.public_signing_key.clone();
        let our_sk = self.secret_signing_key.clone();
        let era_id = block_header.era_id();
        self.executed_block(&block_header);
        let mut effects = if self.is_validator_in(&our_pk, era_id) {
            effect_builder
                .announce_created_finality_signature(FinalitySignature::new(
                    block_header.hash(self.verifiable_chunked_hash_activation()),
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
                let effect = get_switch_blocks(self.chainspec.clone(), effect_builder, new_era_id)
                    .event(move |switch_blocks| Event::CreateNewEra { switch_blocks });
                effects.extend(effect);
            }
        }
        effects
    }

    pub(super) fn handle_deactivate_era<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        era_id: EraId,
        old_faulty_num: usize,
        delay: Duration,
    ) -> Effects<Event> {
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

    pub(super) fn resolve_validity<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        resolve_validity: ResolveValidity,
    ) -> Effects<Event> {
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

    fn handle_consensus_outcomes<REv: ReactorEventT, T>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        outcomes: T,
    ) -> Effects<Event>
    where
        T: IntoIterator<Item = ProtocolOutcome<ClContext>>,
    {
        outcomes
            .into_iter()
            .flat_map(|result| self.handle_consensus_outcome(effect_builder, rng, era_id, result))
            .collect()
    }

    /// Returns `true` if any of the most recent eras has evidence against the validator with key
    /// `pub_key`.
    fn has_evidence(&self, era_id: EraId, pub_key: PublicKey) -> bool {
        self.iter_past(era_id, PAST_EVIDENCE_ERAS)
            .any(|eid| self.era(eid).consensus.has_evidence(&pub_key))
    }

    /// Returns the era with the specified ID. Panics if it does not exist.
    fn era(&self, era_id: EraId) -> &Era {
        &self.open_eras[&era_id]
    }

    /// Returns the era with the specified ID mutably. Panics if it does not exist.
    fn era_mut(&mut self, era_id: EraId) -> &mut Era {
        self.open_eras.get_mut(&era_id).unwrap()
    }

    #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
    fn handle_consensus_outcome<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
        consensus_result: ProtocolOutcome<ClContext>,
    ) -> Effects<Event> {
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
                        effect_builder.enqueue_message(to, message.into()).await;
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
                    .iter_past(era_id, PAST_EVIDENCE_ERAS)
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
                let proposed_block = Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone());
                let finalized_approvals: HashMap<_, _> = proposed_block
                    .deploys()
                    .iter()
                    .chain(proposed_block.transfers().iter())
                    .map(|dwa| {
                        (
                            *dwa.deploy_hash(),
                            FinalizedApprovals::new(dwa.approvals().clone()),
                        )
                    })
                    .collect();
                let finalized_block = FinalizedBlock::new(
                    proposed_block,
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
                effects.extend(
                    execute_finalized_block(effect_builder, finalized_approvals, finalized_block)
                        .ignore(),
                );
                self.update_consensus_pause();
                effects
            }
            ProtocolOutcome::ValidateConsensusValue {
                sender,
                proposed_block,
            } => {
                if era_id.saturating_add(PAST_EVIDENCE_ERAS) < self.current_era
                    || !self.open_eras.contains_key(&era_id)
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
                    effects.extend(effect_builder.send_message(sender, msg.into()).ignore());
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
                for e_id in self.iter_future(era_id, PAST_EVIDENCE_ERAS) {
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
                .iter_past_other(era_id, PAST_EVIDENCE_ERAS)
                .flat_map(|e_id| {
                    self.delegate_to_era(effect_builder, rng, e_id, |consensus, _| {
                        consensus.request_evidence(sender, &pub_key)
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
    ) -> Effects<Event> {
        debug!("got {}", activation_point);
        self.next_upgrade_activation_point = Some(activation_point);
        Effects::new()
    }

    pub(super) fn status(
        &self,
        responder: Responder<Option<(PublicKey, Option<TimeDiff>)>>,
    ) -> Effects<Event> {
        let public_key = self.public_signing_key.clone();
        let round_length = self
            .open_eras
            .get(&self.current_era)
            .and_then(|era| era.consensus.next_round_length());
        responder.respond(Some((public_key, round_length))).ignore()
    }

    fn disconnect<REv: ReactorEventT>(
        &self,
        effect_builder: EffectBuilder<REv>,
        sender: NodeId,
    ) -> Effects<Event> {
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

    /// Get a reference to the era supervisor's open eras.
    pub(crate) fn open_eras(&self) -> &HashMap<EraId, Era> {
        &self.open_eras
    }

    /// Returns the most recent era.
    pub(crate) fn current_era(&self) -> EraId {
        self.current_era
    }
}

#[cfg(test)]
impl EraSupervisor {
    /// Returns this node's validator key.
    pub(crate) fn public_key(&self) -> &PublicKey {
        &self.public_signing_key
    }
}

/// Returns all switch blocks needed to initialize `era_id`.
///
/// Those are the booking block, i.e. the switch block in `era_id - auction_delay - 1`,
/// the key block, i.e. the switch block in `era_id - 1`, and all switch blocks in between.
async fn get_switch_blocks<REv>(
    chainspec: Arc<Chainspec>,
    effect_builder: EffectBuilder<REv>,
    era_id: EraId,
) -> Vec<BlockHeader>
where
    REv: From<StorageRequest>,
{
    let mut switch_blocks = Vec::new();
    let from = chainspec.earliest_switch_block_needed(era_id);
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
            deploys_or_transfer.push(deploy_or_transfer.into_naive())
        } else {
            return None;
        }
    }
    Some(deploys_or_transfer)
}

async fn execute_finalized_block<REv>(
    effect_builder: EffectBuilder<REv>,
    finalized_approvals: HashMap<DeployHash, FinalizedApprovals>,
    finalized_block: FinalizedBlock,
) where
    REv: From<StorageRequest> + From<ControlAnnouncement> + From<ContractRuntimeRequest>,
{
    // if the block exists in storage, it either has been executed before, or we fast synced to a
    // higher block - skip execution
    if effect_builder
        .block_header_exists(finalized_block.height())
        .await
    {
        return;
    }
    for (deploy_hash, finalized_approvals) in finalized_approvals {
        effect_builder
            .store_finalized_approvals(deploy_hash, finalized_approvals)
            .await;
    }
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
fn instance_id(chainspec_hash: Digest, era_id: EraId, key_block_hash: BlockHash) -> Digest {
    Digest::hash_pair(
        key_block_hash.inner().value(),
        Digest::hash_pair(chainspec_hash, era_id.to_le_bytes()).value(),
    )
}

/// Checks that a [BlockPayload] does not have deploys we have already included in blocks in
/// previous eras. This is done by repeatedly querying storage for deploy metadata. When metadata is
/// found storage is queried again to get the era id for the included deploy. That era id must *not*
/// be less than the current era, otherwise the deploy is a replay attack.
async fn check_deploys_for_replay_in_previous_eras_and_validate_block<REv>(
    effect_builder: EffectBuilder<REv>,
    proposed_block_era_id: EraId,
    sender: NodeId,
    proposed_block: ProposedBlock<ClContext>,
) -> Event
where
    REv: From<BlockValidationRequest> + From<StorageRequest>,
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
                sender,
                proposed_block: proposed_block.clone(),
                valid: false,
            });
        }
    }

    let sender_for_validate_block: NodeId = sender;
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
