#![allow(clippy::boxed_local)] // We use boxed locals to pass on event data unchanged.

//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

pub(super) mod debug;
mod era;

use std::{
    cmp,
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::TryInto,
    fmt::{self, Debug, Formatter},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::Error;
use datasize::DataSize;
use futures::{Future, FutureExt};
use itertools::Itertools;
use prometheus::Registry;
use rand::Rng;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use casper_types::{
    AsymmetricType, BlockHash, BlockHeader, Chainspec, ConsensusProtocolName, Deploy, DeployHash,
    Digest, DisplayIter, EraId, PublicKey, RewardedSignatures, TimeDiff, Timestamp,
};

use crate::{
    components::{
        consensus::{
            cl_context::{ClContext, Keypair},
            consensus_protocol::{
                ConsensusProtocol, FinalizedBlock as CpFinalizedBlock, ProposedBlock,
                ProtocolOutcome,
            },
            metrics::Metrics,
            validator_change::{ValidatorChange, ValidatorChanges},
            ActionId, ChainspecConsensusExt, Config, ConsensusMessage, ConsensusRequestMessage,
            Event, HighwayProtocol, NewBlockPayload, ReactorEventT, ResolveValidity, TimerId, Zug,
        },
        network::blocklist::BlocklistJustification,
    },
    effect::{
        announcements::FatalAnnouncement,
        requests::{BlockValidationRequest, ContractRuntimeRequest, StorageRequest},
        AutoClosingResponder, EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal, protocol,
    types::{
        create_single_block_rewarded_signatures, BlockWithMetadata, DeployOrTransferHash,
        ExecutableBlock, FinalizedBlock, FinalizedDeployApprovals, InternalEraReport,
        MetaBlockState, NodeId, ValidatorMatrix,
    },
    NodeRng,
};

pub use self::era::Era;
use crate::components::consensus::error::CreateNewEraError;

use super::{traits::ConsensusNetworkMessage, BlockContext};

/// The delay in milliseconds before we shutdown after the number of faulty validators exceeded the
/// fault tolerance threshold.
const FTT_EXCEEDED_SHUTDOWN_DELAY_MILLIS: u64 = 60 * 1000;
/// A warning is printed if a timer is delayed by more than this.
const TIMER_DELAY_WARNING_MILLIS: u64 = 1000;

/// The number of eras across which evidence can be cited.
/// If this is 1, you can cite evidence from the previous era, but not the one before that.
/// To be able to detect that evidence, we also keep that number of active past eras in memory.
pub(super) const PAST_EVIDENCE_ERAS: u64 = 1;
/// The total number of past eras that are kept in memory in addition to the current one.
/// The more recent half of these is active: it contains units and can still accept further units.
/// The older half is in evidence-only state, and only used to validate cited evidence.
pub(super) const PAST_OPEN_ERAS: u64 = 2 * PAST_EVIDENCE_ERAS;

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
    open_eras: BTreeMap<EraId, Era>,
    validator_matrix: ValidatorMatrix,
    chainspec: Arc<Chainspec>,
    config: Config,
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
    last_progress: Timestamp,
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
    pub(crate) fn new(
        storage_dir: &Path,
        validator_matrix: ValidatorMatrix,
        config: Config,
        chainspec: Arc<Chainspec>,
        registry: &Registry,
    ) -> Result<Self, Error> {
        let unit_files_folder = storage_dir.join("unit_files");
        std::fs::create_dir_all(&unit_files_folder)?;
        info!(our_id = %validator_matrix.public_signing_key(), "EraSupervisor pubkey",);
        let metrics = Metrics::new(registry)?;

        let era_supervisor = Self {
            open_eras: Default::default(),
            validator_matrix,
            chainspec,
            config,
            next_block_height: 0,
            metrics,
            unit_files_folder,
            next_executed_height: 0,
            last_progress: Timestamp::now(),
        };

        Ok(era_supervisor)
    }

    /// Returns whether we are a validator in the current era.
    pub(crate) fn is_active_validator(&self) -> bool {
        if let Some(era_id) = self.current_era() {
            return self.open_eras[&era_id]
                .validators()
                .contains_key(self.validator_matrix.public_signing_key());
        }
        false
    }

    /// Returns the most recent era.
    pub(crate) fn current_era(&self) -> Option<EraId> {
        self.open_eras.keys().last().copied()
    }

    pub(crate) fn create_required_eras<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        recent_switch_block_headers: &[BlockHeader],
    ) -> Option<Effects<Event>> {
        if !recent_switch_block_headers
            .iter()
            .tuple_windows()
            .all(|(b0, b1)| b0.next_block_era_id() == b1.era_id())
        {
            error!("switch block headers are not consecutive; this is a bug");
            return None;
        }

        let highest_switch_block_header = recent_switch_block_headers.last()?;

        let new_era_id = highest_switch_block_header.next_block_era_id();

        // We need to initialize current_era and (evidence-only) current_era - 1.
        // To initialize an era, all switch blocks between its booking block and its key block are
        // required. The booking block for era N is in N - auction_delay - 1, and the key block in
        // N - 1. So we need all switch blocks between:
        // (including) current_era - 1 - auction_delay - 1 and (excluding) current_era.
        // However, we never use any block from before the last activation point.
        //
        // Example: If auction_delay is 1, to initialize era N we need the switch blocks from era N
        // and N - 1. If current_era is 10, we will initialize eras 10 and 9. So we need the switch
        // blocks from eras 9, 8, and 7.
        let earliest_open_era = self.chainspec.earliest_relevant_era(new_era_id);
        let earliest_era = self
            .chainspec
            .earliest_switch_block_needed(earliest_open_era);
        debug_assert!(earliest_era <= new_era_id);

        let earliest_index = recent_switch_block_headers
            .iter()
            .position(|block_header| block_header.era_id() == earliest_era)?;
        let relevant_switch_block_headers = &recent_switch_block_headers[earliest_index..];

        // We initialize the era that `relevant_switch_block_headers` last block is the key
        // block for. We want to initialize the two latest eras, so we have to pass in the whole
        // slice for the current era, and omit one element for the other one. We never initialize
        // the activation era or an earlier era, however.
        //
        // In the example above, we would call create_new_era with the switch blocks from eras
        // 8 and 9 (to initialize 10) and then 7 and 8 (for era 9).
        // (We don't truncate the slice at the start since unneeded blocks are ignored.)
        let mut effects = Effects::new();
        let from = relevant_switch_block_headers
            .len()
            .saturating_sub(PAST_EVIDENCE_ERAS as usize)
            .max(1);
        let old_current_era = self.current_era();
        let now = Timestamp::now();
        for i in (from..=relevant_switch_block_headers.len()).rev() {
            effects.extend(self.create_new_era_effects(
                effect_builder,
                rng,
                &relevant_switch_block_headers[..i],
                now,
            ));
        }
        if self.current_era() != old_current_era {
            effects.extend(self.make_latest_era_current(effect_builder, rng, now));
        }
        effects.extend(self.activate_latest_era_if_needed(effect_builder, rng, now));
        Some(effects)
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

    /// Pauses or unpauses consensus: Whenever the last executed block is too far behind the last
    /// finalized block, we suspend consensus.
    fn update_consensus_pause<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        era_id: EraId,
    ) -> Effects<Event> {
        let paused = self
            .next_block_height
            .saturating_sub(self.next_executed_height)
            > self.config.max_execution_delay;
        self.delegate_to_era(effect_builder, rng, era_id, |consensus, _| {
            consensus.set_paused(paused, Timestamp::now())
        })
    }

    /// Initializes a new era. The switch blocks must contain the most recent `auction_delay + 1`
    /// ones, in order, but at most as far back as to the last activation point.
    pub(super) fn create_new_era_effects<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        switch_blocks: &[BlockHeader],
        now: Timestamp,
    ) -> Effects<Event> {
        match self.create_new_era(switch_blocks, now) {
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

    fn make_latest_era_current<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        now: Timestamp,
    ) -> Effects<Event> {
        let era_id = match self.current_era() {
            Some(era_id) => era_id,
            None => {
                return Effects::new();
            }
        };
        self.metrics
            .consensus_current_era
            .set(era_id.value() as i64);
        let start_height = self.era(era_id).start_height;
        self.next_block_height = self.next_block_height.max(start_height);
        let outcomes = self.era_mut(era_id).consensus.handle_is_current(now);
        self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
    }

    fn activate_latest_era_if_needed<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        now: Timestamp,
    ) -> Effects<Event> {
        let era_id = match self.current_era() {
            Some(era_id) => era_id,
            None => {
                return Effects::new();
            }
        };
        if self.era(era_id).consensus.is_active() {
            return Effects::new();
        }
        let our_id = self.validator_matrix.public_signing_key().clone();
        let outcomes = if !self.era(era_id).validators().contains_key(&our_id) {
            info!(era = era_id.value(), %our_id, "not voting; not a validator");
            vec![]
        } else {
            info!(era = era_id.value(), %our_id, "start voting");
            let secret = Keypair::new(
                self.validator_matrix.secret_signing_key().clone(),
                our_id.clone(),
            );
            let instance_id = self.era(era_id).consensus.instance_id();
            let unit_hash_file = self.unit_file(instance_id);
            self.era_mut(era_id).consensus.activate_validator(
                our_id,
                secret,
                now,
                Some(unit_hash_file),
            )
        };
        self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
    }

    /// Initializes a new era. The switch blocks must contain the most recent `auction_delay + 1`
    /// ones, in order, but at most as far back as to the last activation point.
    fn create_new_era(
        &mut self,
        switch_blocks: &[BlockHeader],
        now: Timestamp,
    ) -> Result<(EraId, Vec<ProtocolOutcome<ClContext>>), CreateNewEraError> {
        let key_block = switch_blocks
            .last()
            .ok_or(CreateNewEraError::AttemptedToCreateEraWithNoSwitchBlocks)?;
        let era_id = key_block.era_id().successor();

        let chainspec_hash = self.chainspec.hash();
        let key_block_hash = key_block.block_hash();
        let instance_id = instance_id(chainspec_hash, era_id, key_block_hash);

        if self.open_eras.contains_key(&era_id) {
            debug!(era = era_id.value(), "era already exists");
            return Ok((era_id, vec![]));
        }

        let era_end = key_block.clone_era_end().ok_or_else(|| {
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

        let validators = era_end.next_era_validator_weights();

        if let Some(current_era) = self.current_era() {
            if current_era > era_id.saturating_add(PAST_EVIDENCE_ERAS) {
                warn!(era = era_id.value(), "trying to create obsolete era");
                return Ok((era_id, vec![]));
            }
        }

        // Compute the seed for the PRNG from the booking block hash and the accumulated seed.
        let auction_delay = self.chainspec.core_config.auction_delay as usize;
        let booking_block_hash =
            if let Some(booking_block) = switch_blocks.iter().rev().nth(auction_delay) {
                booking_block.block_hash()
            } else {
                // If there's no booking block for the `era_id`
                // (b/c it would have been from before Genesis, upgrade or emergency restart),
                // use a "zero" block hash. This should not hurt the security of the leader
                // selection algorithm.
                BlockHash::default()
            };
        let seed = Self::era_seed(booking_block_hash, *key_block.accumulated_seed());

        // The beginning of the new era is marked by the key block.
        #[allow(clippy::integer_arithmetic)] // Block height should never reach u64::MAX.
        let start_height = key_block.height() + 1;
        let start_time = key_block.timestamp();

        // Validators that were inactive in the previous era will be excluded from leader selection
        // in the new era.
        let inactive = era_end.inactive_validators().iter().cloned().collect();

        // Validators that were only exposed as faulty after the booking block are still in the new
        // era's validator set but get banned.
        let blocks_after_booking_block = switch_blocks.iter().rev().take(auction_delay);
        let faulty = blocks_after_booking_block
            .filter_map(|switch_block| switch_block.maybe_equivocators())
            .flat_map(|equivocators| equivocators.iter())
            .cloned()
            .collect();

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
        let (consensus, mut outcomes) = match self.chainspec.core_config.consensus_protocol {
            ConsensusProtocolName::Highway => HighwayProtocol::new_boxed(
                instance_id,
                validators.clone(),
                &faulty,
                &inactive,
                self.chainspec.as_ref(),
                &self.config,
                maybe_prev_era.map(|era| &*era.consensus),
                start_time,
                seed,
                now,
            ),
            ConsensusProtocolName::Zug => Zug::new_boxed(
                instance_id,
                validators.clone(),
                &faulty,
                &inactive,
                self.chainspec.as_ref(),
                &self.config,
                maybe_prev_era.map(|era| &*era.consensus),
                start_time,
                seed,
                now,
                self.unit_file(&instance_id),
            ),
        };

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
        let our_id = self.validator_matrix.public_signing_key().clone();
        if self
            .current_era()
            .map_or(false, |current_era| current_era > era_id)
        {
            trace!(
                era = era_id.value(),
                current_era = ?self.current_era(),
                "not voting; initializing past era"
            );
            // We're creating an era that's not the current era - which means we're currently
            // initializing consensus and we want to set all the older eras to be evidence only.
            if let Some(era) = self.open_eras.get_mut(&era_id) {
                era.consensus.set_evidence_only();
            }
        } else {
            self.metrics
                .consensus_current_era
                .set(era_id.value() as i64);
            self.next_block_height = self.next_block_height.max(start_height);
            outcomes.extend(self.era_mut(era_id).consensus.handle_is_current(now));
            if !self.era(era_id).validators().contains_key(&our_id) {
                info!(era = era_id.value(), %our_id, "not voting; not a validator");
            } else {
                info!(era = era_id.value(), %our_id, "start voting");
                let secret = Keypair::new(
                    self.validator_matrix.secret_signing_key().clone(),
                    our_id.clone(),
                );
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
        if let Some(current_era) = self.current_era() {
            let mut removed_instance_ids = vec![];
            let earliest_open_era = current_era.saturating_sub(PAST_OPEN_ERAS);
            let earliest_active_era = current_era.saturating_sub(PAST_EVIDENCE_ERAS);
            self.open_eras.retain(|era_id, era| {
                if earliest_open_era > *era_id {
                    trace!(era = era_id.value(), "removing obsolete era");
                    removed_instance_ids.push(*era.consensus.instance_id());
                    false
                } else if earliest_active_era > *era_id {
                    trace!(era = era_id.value(), "setting old era to evidence only");
                    era.consensus.set_evidence_only();
                    true
                } else {
                    true
                }
            });
            for instance_id in removed_instance_ids {
                if let Err(err) = fs::remove_file(self.unit_file(&instance_id)) {
                    match err.kind() {
                        io::ErrorKind::NotFound => {}
                        err => warn!(?err, "could not delete unit hash file"),
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
            self.validator_matrix.public_signing_key().to_hex()
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
                self.log_missing_era(era_id);
                Effects::new()
            }
            Some(era) => {
                let outcomes = f(&mut *era.consensus, rng);
                self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes)
            }
        }
    }

    fn log_missing_era(&self, era_id: EraId) {
        let era = era_id.value();
        if let Some(current_era_id) = self.current_era() {
            match era_id.cmp(&current_era_id) {
                cmp::Ordering::Greater => info!(era, "received message for future era"),
                cmp::Ordering::Equal => error!(era, "missing current era"),
                cmp::Ordering::Less => info!(era, "received message for obsolete era"),
            }
        } else {
            info!(era, "received message, but no era initialized");
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
        let now = Timestamp::now();
        let delay = now.saturating_diff(timestamp).millis();
        if delay > TIMER_DELAY_WARNING_MILLIS {
            warn!(
                era = era_id.value(), timer_id = timer_id.0, %delay,
                "timer called with long delay"
            );
        }
        self.delegate_to_era(effect_builder, rng, era_id, move |consensus, rng| {
            consensus.handle_timer(timestamp, now, timer_id, rng)
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
                trace!(era = era_id.value(), "received a consensus message");

                self.delegate_to_era(effect_builder, rng, era_id, move |consensus, rng| {
                    consensus.handle_message(rng, sender, payload, Timestamp::now())
                })
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => match self.current_era() {
                None => Effects::new(),
                Some(current_era) => {
                    if era_id.saturating_add(PAST_EVIDENCE_ERAS) < current_era
                        || !self.open_eras.contains_key(&era_id)
                    {
                        trace!(era = era_id.value(), "not handling message; era too old");
                        return Effects::new();
                    }
                    self.iter_past(era_id, PAST_EVIDENCE_ERAS)
                        .flat_map(|e_id| {
                            self.delegate_to_era(effect_builder, rng, e_id, |consensus, _| {
                                consensus.send_evidence(sender, &pub_key)
                            })
                        })
                        .collect()
                }
            },
        }
    }

    pub(super) fn handle_demand<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        sender: NodeId,
        request: Box<ConsensusRequestMessage>,
        auto_closing_responder: AutoClosingResponder<protocol::Message>,
    ) -> Effects<Event> {
        let ConsensusRequestMessage { era_id, payload } = *request;

        trace!(era = era_id.value(), "received a consensus request");
        match self.open_eras.get_mut(&era_id) {
            None => {
                self.log_missing_era(era_id);
                auto_closing_responder.respond_none().ignore()
            }
            Some(era) => {
                let (outcomes, response) =
                    era.consensus
                        .handle_request_message(rng, sender, payload, Timestamp::now());
                let mut effects =
                    self.handle_consensus_outcomes(effect_builder, rng, era_id, outcomes);
                if let Some(payload) = response {
                    effects.extend(
                        auto_closing_responder
                            .respond(ConsensusMessage::Protocol { era_id, payload }.into())
                            .ignore(),
                    );
                } else {
                    effects.extend(auto_closing_responder.respond_none().ignore());
                }
                effects
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
        match self.current_era() {
            None => {
                warn!("new block payload but no initialized era");
                Effects::new()
            }
            Some(current_era) => {
                if era_id.saturating_add(PAST_EVIDENCE_ERAS) < current_era
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
        }
    }

    pub(super) fn handle_block_added<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        block_header: BlockHeader,
    ) -> Effects<Event> {
        self.next_executed_height = self
            .next_executed_height
            .max(block_header.height().saturating_add(1));
        let era_id = block_header.era_id();
        let mut effects = self.update_consensus_pause(effect_builder, rng, era_id);

        if self
            .current_era()
            .map_or(true, |current_era| era_id < current_era)
        {
            trace!(era = era_id.value(), "executed block in old era");
            return effects;
        }
        if block_header.next_era_validator_weights().is_some() {
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

    /// Will deactivate voting for the current era.
    /// Does nothing if the current era doesn't exist or is inactive already.
    pub(crate) fn deactivate_current_era(&mut self) -> Result<EraId, String> {
        let which_era = self
            .current_era()
            .ok_or_else(|| "attempt to deactivate an era with no eras instantiated!".to_string())?;
        let era = self.era_mut(which_era);
        if false == era.consensus.is_active() {
            debug!(era_id=%which_era, "attempt to deactivate inactive era");
            return Ok(which_era);
        }
        era.consensus.deactivate_validator();
        Ok(which_era)
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
            effects.extend({
                effect_builder
                    .announce_block_peer_with_justification(
                        sender,
                        BlocklistJustification::SentInvalidConsensusValue { era: era_id },
                    )
                    .ignore()
            });
        }
        if self
            .open_eras
            .get_mut(&era_id)
            .map_or(false, |era| era.resolve_validity(&proposed_block, valid))
        {
            effects.extend(
                self.delegate_to_era(effect_builder, rng, era_id, |consensus, _| {
                    consensus.resolve_validity(proposed_block.clone(), valid, Timestamp::now())
                }),
            );
        }
        effects
    }

    pub(crate) fn last_progress(&self) -> Timestamp {
        self.last_progress
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
        let current_era = match self.current_era() {
            Some(current_era) => current_era,
            None => {
                error!("no current era");
                return Effects::new();
            }
        };
        match consensus_result {
            ProtocolOutcome::Disconnect(sender) => {
                warn!(
                    %sender,
                    "disconnecting from the sender of invalid data"
                );
                {
                    effect_builder
                        .announce_block_peer_with_justification(
                            sender,
                            BlocklistJustification::BadConsensusBehavior,
                        )
                        .ignore()
                }
            }
            ProtocolOutcome::CreatedGossipMessage(payload) => {
                let message = ConsensusMessage::Protocol { era_id, payload };
                effect_builder
                    .broadcast_message_to_validators(message.into(), era_id)
                    .ignore()
            }
            ProtocolOutcome::CreatedTargetedMessage(payload, to) => {
                let message = ConsensusMessage::Protocol { era_id, payload };
                effect_builder.enqueue_message(to, message.into()).ignore()
            }
            ProtocolOutcome::CreatedMessageToRandomPeer(payload) => {
                let message = ConsensusMessage::Protocol { era_id, payload };

                async move {
                    let peers = effect_builder.get_fully_connected_peers(1).await;
                    if let Some(to) = peers.into_iter().next() {
                        effect_builder.enqueue_message(to, message.into()).await;
                    }
                }
                .ignore()
            }
            ProtocolOutcome::CreatedRequestToRandomPeer(payload) => {
                let message = ConsensusRequestMessage { era_id, payload };

                async move {
                    let peers = effect_builder.get_fully_connected_peers(1).await;
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
                let signature_rewards_max_delay =
                    self.chainspec.core_config.signature_rewards_max_delay;
                let initial_era_height = self.era(era_id).start_height;
                let current_block_height =
                    initial_era_height + block_context.ancestor_values().len() as u64;
                let minimum_block_height =
                    current_block_height.saturating_sub(signature_rewards_max_delay);

                let awaitable_appendable_block =
                    effect_builder.request_appendable_block(block_context.timestamp());
                let awaitable_blocks_with_metadata = async move {
                    effect_builder
                        .collect_past_blocks_with_metadata(
                            minimum_block_height..current_block_height,
                            false,
                        )
                        .await
                };
                let accusations = self
                    .iter_past(era_id, PAST_EVIDENCE_ERAS)
                    .flat_map(|e_id| self.era(e_id).consensus.validators_with_evidence())
                    .unique()
                    .filter(|pub_key| !self.era(era_id).faulty.contains(pub_key))
                    .cloned()
                    .collect();
                let random_bit = rng.gen();

                let validator_matrix = self.validator_matrix.clone();

                join_2(awaitable_appendable_block, awaitable_blocks_with_metadata).event(
                    move |(appendable_block, maybe_past_blocks_with_metadata)| {
                        let rewarded_signatures = create_rewarded_signatures(
                            &maybe_past_blocks_with_metadata,
                            validator_matrix,
                            &block_context,
                            signature_rewards_max_delay,
                        );

                        let block_payload = Arc::new(appendable_block.into_block_payload(
                            accusations,
                            rewarded_signatures,
                            random_bit,
                        ));

                        Event::NewBlockPayload(NewBlockPayload {
                            era_id,
                            block_payload,
                            block_context,
                        })
                    },
                )
            }
            ProtocolOutcome::FinalizedBlock(CpFinalizedBlock {
                value,
                timestamp,
                relative_height,
                terminal_block_data,
                equivocators,
                proposer,
            }) => {
                if era_id != current_era {
                    debug!(era = era_id.value(), "finalized block in old era");
                    return Effects::new();
                }
                let era = self.open_eras.get_mut(&era_id).unwrap();
                era.add_accusations(&equivocators);
                era.add_accusations(value.accusations());
                // If this is the era's last block, it contains rewards. Everyone who is accused in
                // the block or seen as equivocating via the consensus protocol gets faulty.

                // TODO - add support for the `compute_rewards` chainspec parameter coming from
                // private chain implementation in the 2.0 rewards scheme.
                let _compute_rewards = self.chainspec.core_config.compute_rewards;
                let report = terminal_block_data.map(|tbd| {
                    // If block rewards are disabled, zero them.
                    // if !compute_rewards {
                    //     for reward in tbd.rewards.values_mut() {
                    //         *reward = 0;
                    //     }
                    // }

                    InternalEraReport {
                        equivocators: era.accusations(),
                        inactive_validators: tbd.inactive_validators,
                    }
                });
                let proposed_block = Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone());
                let finalized_approvals: HashMap<_, _> = proposed_block
                    .deploys()
                    .iter()
                    .chain(proposed_block.transfers().iter())
                    .map(|dwa| {
                        (
                            *dwa.deploy_hash(),
                            FinalizedDeployApprovals::new(dwa.approvals().clone()),
                        )
                    })
                    .collect();
                if let Some(era_report) = report.as_ref() {
                    info!(
                        inactive = %DisplayIter::new(&era_report.inactive_validators),
                        faulty = %DisplayIter::new(&era_report.equivocators),
                        era_id = era_id.value(),
                        "era end: inactive and faulty validators"
                    );
                }
                let finalized_block = FinalizedBlock::new(
                    proposed_block,
                    report,
                    timestamp,
                    era_id,
                    era.start_height + relative_height,
                    proposer,
                );
                info!(
                    era_id = finalized_block.era_id.value(),
                    height = finalized_block.height,
                    timestamp = %finalized_block.timestamp,
                    "finalized block"
                );
                self.metrics.finalized_block(&finalized_block);
                // Announce the finalized block.
                let mut effects = effect_builder
                    .announce_finalized_block(finalized_block.clone())
                    .ignore();
                self.next_block_height = self.next_block_height.max(finalized_block.height + 1);
                // Request execution of the finalized block.
                effects.extend(
                    execute_finalized_block(effect_builder, finalized_approvals, finalized_block)
                        .ignore(),
                );
                let effects_from_updating_pause =
                    self.update_consensus_pause(effect_builder, rng, era_id);
                effects.extend(effects_from_updating_pause);
                effects
            }
            ProtocolOutcome::ValidateConsensusValue {
                sender,
                proposed_block,
            } => {
                if era_id.saturating_add(PAST_EVIDENCE_ERAS) < current_era
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
            ProtocolOutcome::HandledProposedBlock(proposed_block) => effect_builder
                .announce_proposed_block(proposed_block)
                .ignore(),
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
                        consensus.send_evidence(sender, &pub_key)
                    })
                })
                .collect(),
            ProtocolOutcome::WeAreFaulty => Default::default(),
            ProtocolOutcome::DoppelgangerDetected => Default::default(),
            ProtocolOutcome::FttExceeded => effect_builder
                .set_timeout(Duration::from_millis(FTT_EXCEEDED_SHUTDOWN_DELAY_MILLIS))
                .then(move |_| fatal!(effect_builder, "too many faulty validators"))
                .ignore(),
        }
    }

    pub(super) fn status(
        &self,
        responder: Responder<Option<(PublicKey, Option<TimeDiff>)>>,
    ) -> Effects<Event> {
        let public_key = self.validator_matrix.public_signing_key().clone();
        let round_length = self
            .open_eras
            .values()
            .last()
            .and_then(|era| era.consensus.next_round_length());
        responder.respond(Some((public_key, round_length))).ignore()
    }

    /// Get a reference to the era supervisor's open eras.
    pub(crate) fn open_eras(&self) -> &BTreeMap<EraId, Era> {
        &self.open_eras
    }

    /// This node's public signing key.
    pub(crate) fn public_key(&self) -> &PublicKey {
        self.validator_matrix.public_signing_key()
    }
}

/// A serialized consensus network message.
///
/// An entirely transparent newtype around raw bytes. Exists solely to avoid accidental
/// double-serialization of network messages, or serialization of unsuitable types.
///
/// Note that this type fixates the encoding for all consensus implementations to one scheme.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
pub(crate) struct SerializedMessage(Vec<u8>);

impl SerializedMessage {
    /// Serialize the given message from a consensus protocol into bytes.
    ///
    /// # Panics
    ///
    /// Will panic if serialization fails (which must never happen -- ensure types are
    /// serializable!).
    pub(crate) fn from_message<T>(msg: &T) -> Self
    where
        T: ConsensusNetworkMessage + Serialize,
    {
        SerializedMessage(bincode::serialize(msg).expect("should serialize message"))
    }

    /// Attempt to deserialize a given type from incoming raw bytes.
    pub(crate) fn deserialize_incoming<T>(&self) -> Result<T, bincode::Error>
    where
        T: ConsensusNetworkMessage + DeserializeOwned,
    {
        bincode::deserialize(&self.0)
    }

    /// Returns the inner raw bytes.
    pub(crate) fn into_raw(self) -> Vec<u8> {
        self.0
    }

    /// Returns a reference to the inner raw bytes.
    pub(crate) fn as_raw(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
impl SerializedMessage {
    /// Deserializes a message into a the given value.
    ///
    /// # Panics
    ///
    /// Will panic if deserialization fails.
    #[track_caller]
    pub(crate) fn deserialize_expect<T>(&self) -> T
    where
        T: ConsensusNetworkMessage + DeserializeOwned,
    {
        self.deserialize_incoming()
            .expect("could not deserialize valid zug message from serialized message")
    }
}

async fn get_deploys<REv>(
    effect_builder: EffectBuilder<REv>,
    hashes: Vec<DeployHash>,
) -> Option<Vec<Deploy>>
where
    REv: From<StorageRequest>,
{
    effect_builder
        .get_deploys_from_storage(hashes)
        .await
        .into_iter()
        .map(|maybe_deploy| maybe_deploy.map(|deploy| deploy.into_naive()))
        .collect()
}

async fn execute_finalized_block<REv>(
    effect_builder: EffectBuilder<REv>,
    finalized_approvals: HashMap<DeployHash, FinalizedDeployApprovals>,
    finalized_block: FinalizedBlock,
) where
    REv: From<StorageRequest> + From<FatalAnnouncement> + From<ContractRuntimeRequest>,
{
    for (deploy_hash, finalized_approvals) in finalized_approvals {
        effect_builder
            .store_finalized_approvals(deploy_hash.into(), finalized_approvals.into())
            .await;
    }
    // Get all deploys in order they appear in the finalized block.
    let deploys = match get_deploys(
        effect_builder,
        finalized_block
            .deploy_and_transfer_hashes()
            .cloned()
            .collect_vec(),
    )
    .await
    {
        Some(deploys) => deploys,
        None => {
            fatal!(
                effect_builder,
                "Could not fetch deploys and transfers for finalized block: {:?}",
                finalized_block
            )
            .await;
            return;
        }
    };

    let executable_block =
        ExecutableBlock::from_finalized_block_and_deploys(finalized_block, deploys);
    effect_builder
        .enqueue_block_for_execution(executable_block, MetaBlockState::new())
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

/// When `async move { join!(…) }` is used inline, it prevents rustfmt
/// to run on the chained `event` block.
async fn join_2<T: Future, U: Future>(
    t: T,
    u: U,
) -> (<T as Future>::Output, <U as Future>::Output) {
    futures::join!(t, u)
}

fn create_rewarded_signatures(
    maybe_past_blocks_with_metadata: &[Option<BlockWithMetadata>],
    validator_matrix: ValidatorMatrix,
    block_context: &BlockContext<ClContext>,
    signature_rewards_max_delay: u64,
) -> RewardedSignatures {
    let num_ancestor_values = block_context.ancestor_values().len();
    let mut rewarded_signatures =
        RewardedSignatures::new(maybe_past_blocks_with_metadata.iter().rev().map(
            |maybe_past_block_with_metadata| {
                maybe_past_block_with_metadata
                    .as_ref()
                    .and_then(|past_block_with_metadata| {
                        create_single_block_rewarded_signatures(
                            &validator_matrix,
                            past_block_with_metadata,
                        )
                    })
                    .unwrap_or_default()
            },
        ));

    // exclude the signatures that were already included in ancestor blocks
    for (past_index, ancestor_rewarded_signatures) in block_context
        .ancestor_values()
        .iter()
        .map(|value| value.rewarded_signatures().clone())
        // the above will only cover the signatures from the same era - chain
        // with signatures from the blocks read from storage
        .chain(
            maybe_past_blocks_with_metadata
                .iter()
                .rev()
                // skip the blocks corresponding to heights covered by
                // ancestor_values
                .skip(num_ancestor_values)
                .map(|maybe_past_block| {
                    maybe_past_block.as_ref().map_or_else(
                        // if we're missing a block, this could cause us to include duplicate
                        // signatures and make our proposal invalid - but this is covered by the
                        // requirement for a validator to have blocks spanning the max deploy TTL
                        // in the past
                        Default::default,
                        |past_block| past_block.block.rewarded_signatures().clone(),
                    )
                }),
        )
        .enumerate()
        .take(signature_rewards_max_delay as usize)
    {
        rewarded_signatures = rewarded_signatures
            .difference(&ancestor_rewarded_signatures.left_padded(past_index.saturating_add(1)));
    }

    rewarded_signatures
}
