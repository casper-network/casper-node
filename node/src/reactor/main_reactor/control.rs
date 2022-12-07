use itertools::Itertools;
use std::time::Duration;
use tracing::{debug, info, trace};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::{
        block_synchronizer,
        block_synchronizer::BlockSynchronizerProgress,
        consensus::EraReport,
        contract_runtime::ExecutionPreState,
        deploy_buffer::{self, DeployBuffer},
        diagnostics_port, event_stream_server, network, rest_server, rpc_server, upgrade_watcher,
        InitializedComponent,
    },
    effect::{EffectBuilder, EffectExt, Effects},
    fatal,
    reactor::main_reactor::{
        catch_up::CatchUpInstruction, keep_up::KeepUpInstruction,
        upgrade_shutdown::UpgradeShutdownInstruction, upgrading_instruction::UpgradingInstruction,
        utils, validate::ValidateInstruction, MainEvent, MainReactor, ReactorState,
    },
    types::{BlockHash, BlockPayload, FinalizedBlock, Item},
    NodeRng,
};

/// Cranking delay when encountered a non-switch block when checking the validator status.
const VALIDATION_STATUS_DELAY_FOR_NON_SWITCH_BLOCK: Duration = Duration::from_secs(2);

/// Allow the runner to shut down cleanly before shutting down the reactor.

impl MainReactor {
    pub(super) fn crank(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Effects<MainEvent> {
        if self.attempts > self.max_attempts {
            return fatal!(effect_builder, "exceeded reattempt tolerance").ignore();
        }
        let (delay, mut effects) = self.do_crank(effect_builder, rng);
        effects.extend(
            async move {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await
                }
            }
            .event(|_| MainEvent::ReactorCrank),
        );
        effects
    }

    fn do_crank(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> (Duration, Effects<MainEvent>) {
        match self.state {
            ReactorState::Initialize => match self.initialize_next_component(effect_builder) {
                Some(effects) => (Duration::ZERO, effects),
                None => {
                    if false == self.net.has_sufficient_fully_connected_peers() {
                        info!("Initialize: awaiting sufficient fully-connected peers");
                        return (Duration::from_secs(2), Effects::new());
                    }
                    if let Err(msg) = self.refresh_contract_runtime() {
                        return (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore());
                    }
                    info!("Initialize: switch to CatchUp");
                    self.state = ReactorState::CatchUp;
                    (Duration::ZERO, Effects::new())
                }
            },
            ReactorState::Upgrading => match self.upgrading_instruction() {
                UpgradingInstruction::Fatal(msg) => {
                    (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                }
                UpgradingInstruction::CheckLater(msg, wait) => {
                    debug!("Upgrading: {}", msg);
                    (wait, Effects::new())
                }
                UpgradingInstruction::CatchUp => {
                    info!("Upgrading: switch to CatchUp");
                    self.state = ReactorState::CatchUp;
                    (Duration::ZERO, Effects::new())
                }
            },
            ReactorState::CatchUp => match self.catch_up_instruction(effect_builder, rng) {
                CatchUpInstruction::Fatal(msg) => {
                    (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                }
                CatchUpInstruction::ShutdownForUpgrade => {
                    info!("CatchUp: shutting down for upgrade");
                    self.state = ReactorState::ShutdownForUpgrade;
                    (Duration::ZERO, Effects::new())
                }
                CatchUpInstruction::CommitGenesis => match self.commit_genesis(effect_builder) {
                    Ok(effects) => {
                        info!("CatchUp: switch to Validate at genesis");
                        self.state = ReactorState::Validate;
                        self.block_synchronizer.pause();
                        (Duration::ZERO, effects)
                    }
                    Err(msg) => (
                        Duration::ZERO,
                        fatal!(effect_builder, "failed to commit genesis: {}", msg).ignore(),
                    ),
                },
                CatchUpInstruction::CommitUpgrade => match self.commit_upgrade(effect_builder) {
                    Ok(effects) => {
                        info!("CatchUp: switch to Upgrading");
                        self.state = ReactorState::Upgrading;
                        (Duration::ZERO, effects)
                    }
                    Err(msg) => (
                        Duration::ZERO,
                        fatal!(effect_builder, "failed to commit upgrade: {}", msg).ignore(),
                    ),
                },
                CatchUpInstruction::CheckLater(msg, wait) => {
                    debug!("CatchUp: {}", msg);
                    (wait, Effects::new())
                }
                CatchUpInstruction::Do(wait, effects) => {
                    debug!("CatchUp: node is processing effects");
                    (wait, effects)
                }
                CatchUpInstruction::CaughtUp => {
                    if let Err(msg) = self.refresh_contract_runtime() {
                        return (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore());
                    }
                    info!("CatchUp: switch to KeepUp");
                    self.state = ReactorState::KeepUp;
                    (Duration::ZERO, Effects::new())
                }
            },
            ReactorState::KeepUp => match self.keep_up_instruction(effect_builder, rng) {
                KeepUpInstruction::Fatal(msg) => {
                    (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                }
                KeepUpInstruction::ShutdownForUpgrade => {
                    info!("KeepUp: switch to ShutdownForUpgrade");
                    self.state = ReactorState::ShutdownForUpgrade;
                    (Duration::ZERO, Effects::new())
                }
                KeepUpInstruction::CheckLater(msg, wait) => {
                    debug!("KeepUp: {}", msg);
                    (wait, Effects::new())
                }
                KeepUpInstruction::Do(wait, effects) => {
                    debug!("KeepUp: node is processing effects");
                    (wait, effects)
                }
                KeepUpInstruction::CatchUp => {
                    info!("KeepUp: switch to CatchUp");
                    self.state = ReactorState::CatchUp;
                    (Duration::ZERO, Effects::new())
                }
                KeepUpInstruction::Validate(effects) => {
                    // node is in validator set and consensus has what it needs to validate
                    info!("KeepUp: switch to Validate");
                    self.state = ReactorState::Validate;
                    self.block_synchronizer.pause();
                    (Duration::ZERO, effects)
                }
            },
            ReactorState::Validate => match self.validate_instruction(effect_builder, rng) {
                ValidateInstruction::Fatal(msg) => {
                    (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                }
                ValidateInstruction::ShutdownForUpgrade => {
                    info!("Validate: switch to ShutdownForUpgrade");
                    self.state = ReactorState::ShutdownForUpgrade;
                    (Duration::ZERO, Effects::new())
                }
                ValidateInstruction::NonSwitchBlock => {
                    (VALIDATION_STATUS_DELAY_FOR_NON_SWITCH_BLOCK, Effects::new())
                }
                ValidateInstruction::CheckLater(msg, wait) => {
                    debug!("Validate: {}", msg);
                    (wait, Effects::new())
                }
                ValidateInstruction::Do(wait, effects) => {
                    trace!("Validate: node is processing effects");
                    (wait, effects)
                }
                ValidateInstruction::KeepUp => {
                    info!("Validate: switch to KeepUp");
                    self.state = ReactorState::KeepUp;
                    self.block_synchronizer.resume();
                    (Duration::ZERO, Effects::new())
                }
            },
            ReactorState::ShutdownForUpgrade => {
                match self.upgrade_shutdown_instruction(effect_builder) {
                    UpgradeShutdownInstruction::Fatal(msg) => (
                        Duration::ZERO,
                        fatal!(effect_builder, "ShutdownForUpgrade: {}", msg).ignore(),
                    ),
                    UpgradeShutdownInstruction::CheckLater(msg, wait) => {
                        debug!("ShutdownForUpgrade: {}", msg);
                        (wait, Effects::new())
                    }
                    UpgradeShutdownInstruction::Do(wait, effects) => {
                        trace!("ShutdownForUpgrade: node is processing effects");
                        (wait, effects)
                    }
                }
            }
        }
    }

    fn initialize_next_component(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Option<Effects<MainEvent>> {
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.diagnostics_port,
            MainEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.upgrade_watcher,
            MainEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.event_stream_server,
            MainEvent::EventStreamServer(event_stream_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.rest_server,
            MainEvent::RestServer(rest_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.block_synchronizer,
            MainEvent::BlockSynchronizer(block_synchronizer::Event::Initialize),
        ) {
            return Some(effects);
        }
        if <DeployBuffer as InitializedComponent<MainEvent>>::is_uninitialized(&self.deploy_buffer)
        {
            let timestamp = self.recent_switch_block_headers.last().map_or_else(
                Timestamp::now,
                |switch_block| {
                    switch_block
                        .timestamp()
                        .saturating_sub(self.chainspec.deploy_config.max_ttl)
                },
            );
            let blocks = match self.storage.read_blocks_since(timestamp) {
                Ok(blocks) => blocks,
                Err(err) => {
                    return Some(
                        fatal!(
                            effect_builder,
                            "fatal block store error when attempting to read highest blocks: {}",
                            err
                        )
                        .ignore(),
                    )
                }
            };
            debug!(
                blocks = ?blocks.iter().map(|b| b.height()).collect_vec(),
                "DeployBuffer: initialization"
            );
            let event = deploy_buffer::Event::Initialize(blocks);
            if let Some(effects) = utils::initialize_component(
                effect_builder,
                &mut self.deploy_buffer,
                MainEvent::DeployBuffer(event),
            ) {
                return Some(effects);
            }
        }
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.rpc_server,
            MainEvent::RpcServer(rpc_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.net,
            MainEvent::Network(network::Event::Initialize),
        ) {
            return Some(effects);
        }
        None
    }

    fn commit_genesis(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Result<Effects<MainEvent>, String> {
        let post_state_hash = match self.contract_runtime.commit_genesis(
            self.chainspec.clone().as_ref(),
            self.chainspec_raw_bytes.clone().as_ref(),
        ) {
            Ok(success) => success.post_state_hash,
            Err(error) => {
                return Err(error.to_string());
            }
        };

        let genesis_timestamp = match self
            .chainspec
            .protocol_config
            .activation_point
            .genesis_timestamp()
        {
            None => {
                return Err("must have genesis timestamp".to_string());
            }
            Some(timestamp) => timestamp,
        };

        info!(
            %post_state_hash,
            %genesis_timestamp,
            network_name = %self.chainspec.network_config.name,
            "successfully ran genesis"
        );

        let next_block_height = 0;
        self.initialize_contract_runtime(
            next_block_height,
            post_state_hash,
            BlockHash::default(),
            Digest::default(),
        )?;

        let finalized_block = FinalizedBlock::new(
            BlockPayload::default(),
            Some(EraReport::default()),
            genesis_timestamp,
            EraId::default(),
            next_block_height,
            PublicKey::System,
        );
        Ok(effect_builder
            .enqueue_block_for_execution(finalized_block, vec![])
            .ignore())
    }

    fn upgrading_instruction(&self) -> UpgradingInstruction {
        UpgradingInstruction::should_commit_upgrade(
            self.should_commit_upgrade(),
            self.control_logic_default_delay.into(),
        )
    }

    fn commit_upgrade(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Result<Effects<MainEvent>, String> {
        info!("{:?}: committing upgrade", self.state);
        let previous_block_header = match &self.switch_block {
            None => {
                return Err("switch_block should be Some".to_string());
            }
            Some(header) => header.clone(),
        };

        match self.chainspec.ee_upgrade_config(
            *previous_block_header.state_root_hash(),
            previous_block_header.protocol_version(),
            previous_block_header.era_id(),
            self.chainspec_raw_bytes.clone(),
        ) {
            Ok(cfg) => match self.contract_runtime.commit_upgrade(cfg) {
                Ok(success) => {
                    let post_state_hash = success.post_state_hash;
                    info!(
                        network_name = %self.chainspec.network_config.name,
                        %post_state_hash,
                        "upgrade committed"
                    );

                    let next_block_height = previous_block_header.height() + 1;
                    self.initialize_contract_runtime(
                        next_block_height,
                        post_state_hash,
                        previous_block_header.block_hash(),
                        previous_block_header.accumulated_seed(),
                    )?;

                    let finalized_block = FinalizedBlock::new(
                        BlockPayload::default(),
                        Some(EraReport::default()),
                        previous_block_header.timestamp(),
                        previous_block_header.next_block_era_id(),
                        next_block_height,
                        PublicKey::System,
                    );
                    Ok(effect_builder
                        .enqueue_block_for_execution(finalized_block, vec![])
                        .ignore())
                }
                Err(err) => Err(err.to_string()),
            },
            Err(msg) => Err(msg),
        }
    }

    pub(super) fn should_shutdown_for_upgrade(&self) -> bool {
        if let Some(block_header) = self.recent_switch_block_headers.last() {
            return self
                .upgrade_watcher
                .should_upgrade_after(block_header.era_id());
        }
        false
    }

    pub(super) fn should_commit_upgrade(&self) -> Result<bool, String> {
        let highest_switch_block_header = match &self.switch_block {
            None => {
                return Ok(false);
            }
            Some(header) => header,
        };

        if !self
            .chainspec
            .protocol_config
            .is_last_block_before_activation(highest_switch_block_header)
        {
            return Ok(false);
        }

        match self
            .chainspec
            .is_in_modified_validator_set(self.consensus.public_key())
        {
            None => match highest_switch_block_header.next_era_validator_weights() {
                None => Err("switch_block should have next era validator weights".to_string()),
                Some(next_era_validator_weights) => {
                    Ok(next_era_validator_weights.contains_key(self.consensus.public_key()))
                }
            },
            Some(is_validator) => Ok(is_validator),
        }
    }

    pub(super) fn should_validate(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> (bool, Option<Effects<MainEvent>>) {
        // if on a switch block, check if we should go to Validate
        match self.create_required_eras(effect_builder, rng) {
            Err(msg) => {
                return (false, Some(fatal!(effect_builder, "{}", msg).ignore()));
            }
            Ok(Some(effects)) => {
                return (true, Some(effects));
            }
            Ok(None) => (),
        }
        (false, None)
    }

    fn refresh_contract_runtime(&mut self) -> Result<(), String> {
        match self.storage.read_highest_complete_block() {
            Ok(Some(block)) => {
                let block_height = block.height();
                let state_root_hash = block.state_root_hash();
                let block_hash = block.id();
                let accumulated_seed = block.header().accumulated_seed();
                self.initialize_contract_runtime(
                    block_height + 1,
                    *state_root_hash,
                    block_hash,
                    accumulated_seed,
                )
            }
            Ok(None) => {
                Ok(()) // noop
            }
            Err(error) => Err(format!("failed to read highest complete block: {}", error)),
        }
    }

    fn initialize_contract_runtime(
        &mut self,
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) -> Result<(), String> {
        // a better approach might be to have an announcement for immediate switch block
        // creation, which the contract runtime handles and sets itself into
        // the proper state to handle the unexpected block.
        // in the meantime, this is expedient.
        let initial_pre_state = ExecutionPreState::new(
            next_block_height,
            pre_state_root_hash,
            parent_hash,
            parent_seed,
        );
        self.contract_runtime
            .set_initial_state(initial_pre_state)
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    pub(super) fn update_last_progress(
        &mut self,
        block_synchronizer_progress: &BlockSynchronizerProgress,
        is_sync_back: bool,
    ) {
        if let BlockSynchronizerProgress::Syncing(_, _, last_progress) = block_synchronizer_progress
        {
            // do idleness / reattempt checking
            let sync_progress = *last_progress;
            if sync_progress > self.last_progress {
                self.last_progress = sync_progress;
                // if any progress has been made, reset attempts
                self.attempts = 0;
                let state = if is_sync_back {
                    "Historical".to_string()
                } else {
                    format!("{}", self.state)
                };
                debug!(
                    "{}: last_progress: {} {}",
                    state, self.last_progress, block_synchronizer_progress
                );
            }
            if self.last_progress.elapsed() > self.idle_tolerance {
                self.attempts += 1;
            }
        }
    }

    pub(crate) fn update_highest_switch_block(&mut self) -> Result<(), String> {
        let maybe_highest_switch_block_header =
            match self.storage.read_highest_switch_block_headers(1) {
                Ok(highest_switch_block_header) => highest_switch_block_header,
                Err(err) => return Err(err.to_string()),
            };
        self.switch_block = maybe_highest_switch_block_header.first().cloned();
        Ok(())
    }
}
