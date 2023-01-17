use std::time::Duration;
use tracing::{debug, error, info, trace};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey};

use crate::{
    components::{
        block_synchronizer, block_synchronizer::BlockSynchronizerProgress, consensus::EraReport,
        contract_runtime::ExecutionPreState, diagnostics_port, event_stream_server, network,
        rest_server, rpc_server, upgrade_watcher,
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
                Some(effects) => (self.control_logic_default_delay.into(), effects),
                None => {
                    if false == self.net.has_sufficient_fully_connected_peers() {
                        info!("Initialize: awaiting sufficient fully-connected peers");
                        return (self.control_logic_default_delay.into(), Effects::new());
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
                    // purge to avoid polluting the status endpoints w/ stale state
                    self.block_synchronizer.purge();
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
                    self.block_synchronizer.purge();
                    info!("KeepUp: switch to CatchUp");
                    self.state = ReactorState::CatchUp;
                    (Duration::ZERO, Effects::new())
                }
                KeepUpInstruction::Validate(effects) => {
                    // purge to avoid polluting the status endpoints w/ stale state
                    self.block_synchronizer.purge();
                    info!("KeepUp: switch to Validate");
                    self.state = ReactorState::Validate;
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

    // NOTE: the order in which components are initialized is purposeful,
    // so don't alter the order without understanding the semantics
    fn initialize_next_component(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Option<Effects<MainEvent>> {
        // open the diagnostic port first to make sure it can bind & to be responsive during init.
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.diagnostics_port,
            MainEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
        ) {
            return Some(effects);
        }
        // init event stream to make sure it can bind & allow early client connection
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.event_stream_server,
            MainEvent::EventStreamServer(event_stream_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        // init upgrade watcher to make sure we have file access & to observe possible upgrade
        // this should be init'd before the rest & rpc servers as the status endpoints include
        // detected upgrade info.
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.upgrade_watcher,
            MainEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
        ) {
            return Some(effects);
        }

        // initialize deploy buffer from local storage; on a new node this is nearly a noop
        // but on a restarting node it can be relatively time consuming (depending upon TTL and
        // how many deploys there have been within the TTL)
        if let Some(effects) = self
            .deploy_buffer
            .initialize_component(effect_builder, &self.storage)
        {
            return Some(effects);
        }

        // bring up networking near-to-last to avoid unnecessary premature connectivity
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.net,
            MainEvent::Network(network::Event::Initialize),
        ) {
            return Some(effects);
        }

        // bring up the BlockSynchronizer after Network to start it's self-perpetuating
        // dishonest peer announcing behavior
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.block_synchronizer,
            MainEvent::BlockSynchronizer(block_synchronizer::Event::Initialize),
        ) {
            return Some(effects);
        }

        // bring up rpc and rest server last to defer complications (such as put_deploy) and
        // for it to be able to answer to /status, which requires various other components to be
        // initialized
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.rpc_server,
            MainEvent::RpcServer(rpc_server::Event::Initialize),
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

        None
    }

    fn commit_genesis(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Result<Effects<MainEvent>, String> {
        let post_state_hash = match self.contract_runtime.apply_and_commit_genesis(
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
            self.chainspec.protocol_config.activation_point.era_id(),
            self.chainspec_raw_bytes.clone(),
        ) {
            Ok(cfg) => match self.contract_runtime.apply_and_commit_upgrade(cfg) {
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
        let recent_switch_block_headers = match self.storage.read_highest_switch_block_headers(1) {
            Ok(headers) => headers,
            Err(error) => {
                error!("error getting recent switch block headers: {}", error);
                return false;
            }
        };

        if let Some(block_header) = recent_switch_block_headers.last() {
            return self
                .upgrade_watcher
                .should_upgrade_after(block_header.era_id());
        }
        false
    }

    pub(super) fn should_commit_upgrade(&self) -> bool {
        let highest_switch_block_header = match &self.switch_block {
            None => {
                return false;
            }
            Some(header) => header,
        };

        self.chainspec
            .protocol_config
            .is_last_block_before_activation(highest_switch_block_header)
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
