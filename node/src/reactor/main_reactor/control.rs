use std::time::Duration;
use tracing::{debug, error, info, trace, warn};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, TimeDiff, Timestamp};

use crate::{
    components::{
        block_accumulator::{StartingWith, SyncInstruction},
        block_synchronizer,
        block_synchronizer::BlockSynchronizerProgress,
        consensus::EraReport,
        contract_runtime::ExecutionPreState,
        deploy_buffer::{self, DeployBuffer},
        diagnostics_port, event_stream_server, rest_server, rpc_server, small_network, sync_leaper,
        sync_leaper::LeapStatus,
        upgrade_watcher, InitializedComponent, ValidatorBoundComponent,
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor::{
        self,
        main_reactor::{
            catch_up_instruction::CatchUpInstruction, keep_up_instruction::KeepUpInstruction,
            utils, validate_instruction::ValidateInstruction, MainEvent, MainReactor, ReactorState,
        },
    },
    types::{ActivationPoint, BlockHash, BlockHeader, BlockPayload, FinalizedBlock, Item},
    NodeRng,
};

impl MainReactor {
    pub(super) fn crank(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Effects<MainEvent> {
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
            ReactorState::Initialize => match self.initialize_next_component() {
                Some(effects) => (Duration::ZERO, effects),
                None => {
                    info!("Initialize: switch to CatchUp");
                    self.state = ReactorState::CatchUp;
                    (Duration::ZERO, Effects::new())
                }
            },
            ReactorState::CatchUp => match self.catch_up_instruction(effect_builder, rng) {
                CatchUpInstruction::CommitGenesis => match self.commit_genesis(effect_builder) {
                    Ok(effects) => {
                        info!("CatchUp: switch to Validate at genesis");
                        self.state = ReactorState::Validate;
                        (Duration::ZERO, effects)
                    }
                    Err(msg) => (Duration::ZERO, utils::new_shutdown_effect(msg)),
                },
                CatchUpInstruction::CaughtUp => {
                    info!("CatchUp: switch to KeepUp");
                    self.state = ReactorState::KeepUp;
                    (Duration::ZERO, Effects::new())
                }
                CatchUpInstruction::Shutdown(msg) => {
                    (Duration::ZERO, utils::new_shutdown_effect(msg))
                }
                CatchUpInstruction::Do(wait, effects) => {
                    debug!("CatchUp: node is processing effects");
                    (wait, effects)
                }
                CatchUpInstruction::CheckLater(msg, wait) => {
                    debug!("CatchUp: {}", msg);
                    (wait, Effects::new())
                }
            },
            ReactorState::KeepUp => {
                match self.keep_up_instruction(effect_builder, rng) {
                    KeepUpInstruction::Validate(effects) => {
                        // node is in validator set and consensus has what it needs to validate
                        info!("KeepUp: switch to Validate");
                        self.state = ReactorState::Validate;
                        self.block_synchronizer.pause();
                        (Duration::ZERO, effects)
                    }
                    KeepUpInstruction::CatchUp => {
                        info!("KeepUp: switch to CatchUp");
                        self.state = ReactorState::CatchUp;
                        (Duration::ZERO, Effects::new())
                    }
                    KeepUpInstruction::Do(wait, effects) => {
                        debug!("KeepUp: node is processing effects");
                        (wait, effects)
                    }
                    KeepUpInstruction::CheckLater(msg, wait) => {
                        debug!("KeepUp: {}", msg);
                        (wait, Effects::new())
                    }
                }
            }
            ReactorState::Validate => match self.validate_instruction(effect_builder, rng) {
                ValidateInstruction::NonSwitchBlock => (Duration::from_secs(2), Effects::new()),
                ValidateInstruction::KeepUp => {
                    info!("Validate: switch to KeepUp");
                    self.state = ReactorState::KeepUp;
                    self.block_synchronizer.resume();
                    (Duration::ZERO, Effects::new())
                }
                ValidateInstruction::Do(wait, effects) => {
                    trace!("Validate: node is processing effects");
                    (wait, effects)
                }
                ValidateInstruction::CheckLater(msg, wait) => {
                    debug!("Validate: {}", msg);
                    (wait, Effects::new())
                }
            },
            ReactorState::Upgrade => {
                // noop ... patiently wait for the runner to shut down cleanly
                (Duration::from_secs(1), Effects::new())
            }
        }
    }

    fn initialize_next_component(&mut self) -> Option<Effects<MainEvent>> {
        if let Some(effects) = utils::initialize_component(
            &mut self.diagnostics_port,
            "diagnostics",
            MainEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            &mut self.upgrade_watcher,
            "upgrade_watcher",
            MainEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            &mut self.small_network,
            "small_network",
            MainEvent::Network(small_network::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            &mut self.event_stream_server,
            "event_stream_server",
            MainEvent::EventStreamServer(event_stream_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            &mut self.rest_server,
            "rest_server",
            MainEvent::RestServer(rest_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            &mut self.rpc_server,
            "rpc_server",
            MainEvent::RpcServer(rpc_server::Event::Initialize),
        ) {
            return Some(effects);
        }
        if let Some(effects) = utils::initialize_component(
            &mut self.block_synchronizer,
            "block_synchronizer",
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
                    return Some(utils::new_shutdown_effect(format!(
                        "fatal block store error when attempting to read highest \
                                    blocks: {}",
                        err
                    )))
                }
            };
            let event = deploy_buffer::Event::Initialize(blocks);
            if let Some(effects) = utils::initialize_component(
                &mut self.deploy_buffer,
                "deploy_buffer",
                MainEvent::DeployBuffer(event),
            ) {
                return Some(effects);
            }
        }
        None
    }

    fn catch_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> CatchUpInstruction {
        let block_sync_progress = self.block_synchronizer.catch_up_progress();
        debug!("CatchUp: {:?}", block_sync_progress);
        let starting_with = match block_sync_progress {
            BlockSynchronizerProgress::Idle => {
                match self.trusted_hash {
                    None => {
                        // no trusted hash provided use local tip if available
                        match self.storage.read_highest_complete_block() {
                            Ok(Some(block)) => {
                                // -+ : leap w/ local tip
                                info!("CatchUp: local tip detected, no trusted hash");
                                if block.header().is_switch_block() {
                                    self.switch_block = Some(block.id());
                                }
                                StartingWith::LocalTip(*block.hash(), block.height())
                            }
                            Ok(None) if self.switch_block.is_none() => {
                                if let ActivationPoint::Genesis(timestamp) =
                                    self.chainspec.protocol_config.activation_point
                                {
                                    let timediff = timestamp.saturating_diff(Timestamp::now());
                                    if timediff > TimeDiff::default() {
                                        return CatchUpInstruction::CheckLater(
                                            "CatchUp: waiting for genesis activation point"
                                                .to_string(),
                                            Duration::from(timediff),
                                        );
                                    } else {
                                        match self.chainspec.network_config.is_genesis_validator(
                                            self.validator_matrix.public_signing_key(),
                                        ) {
                                            Some(true) => {
                                                info!("CatchUp: genesis validator");
                                                return CatchUpInstruction::CommitGenesis;
                                            }
                                            Some(false) | None => {
                                                info!("CatchUp: genesis non-validator");
                                            }
                                        }
                                    }
                                }
                                // -- : no trusted hash, no local block
                                return CatchUpInstruction::Shutdown(
                                    "CatchUp: cannot proceed without trusted hash".to_string(),
                                );
                            }
                            Ok(None) => {
                                debug!("CatchUp: waiting to store genesis immediate switch block");
                                return CatchUpInstruction::CheckLater(
                                    "CatchUp: waiting for genesis immediate switch block to be stored"
                                        .to_string(),
                                    Duration::from_secs(1),
                                );
                            }
                            Err(err) => {
                                return CatchUpInstruction::Shutdown(format!(
                                    "CatchUp: fatal block store error when attempting to read highest \
                                    complete block: {}",
                                    err
                                ));
                            }
                        }
                    }
                    Some(trusted_hash) => {
                        // if we have a trusted hash and we have a local tip, use the higher
                        match self.storage.read_block_header(&trusted_hash) {
                            Ok(Some(trusted_header)) => {
                                match self.storage.read_highest_complete_block() {
                                    Ok(Some(block)) => {
                                        // ++ : leap w/ the higher of local tip or trusted hash
                                        let trusted_height = trusted_header.height();
                                        if trusted_height > block.height() {
                                            StartingWith::BlockIdentifier(
                                                trusted_hash,
                                                trusted_height,
                                            )
                                        } else {
                                            StartingWith::BlockIdentifier(
                                                *block.hash(),
                                                block.height(),
                                            )
                                        }
                                    }
                                    Ok(None) => {
                                        // should be unreachable if we've gotten this far
                                        StartingWith::Hash(trusted_hash)
                                    }
                                    Err(_) => {
                                        return CatchUpInstruction::Shutdown(
                                            "CatchUp: fatal block store error when attempting to read highest \
                                    complete block"
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                            Ok(None) => {
                                // +- : leap w/ config hash
                                StartingWith::Hash(trusted_hash)
                            }
                            Err(err) => {
                                return CatchUpInstruction::Shutdown(format!(
                                    "CatchUp: fatal block store error when attempting to read highest \
                            complete block: {}",
                                    err
                                ));
                            }
                        }
                    }
                }
            }
            BlockSynchronizerProgress::Syncing(block_hash, maybe_block_height, last_progress) => {
                // do idleness / reattempt checking
                if Timestamp::now().saturating_diff(last_progress) > self.idle_tolerance {
                    self.attempts += 1;
                    if self.attempts > self.max_attempts {
                        return CatchUpInstruction::Shutdown(
                            "CatchUp: block sync idleness exceeded reattempt tolerance".to_string(),
                        );
                    }
                }
                // if any progress has been made, reset attempts
                if last_progress > self.last_progress {
                    debug!("CatchUp: syncing last_progress: {}", last_progress);
                    self.last_progress = last_progress;
                    self.attempts = 0;
                }
                match maybe_block_height {
                    None => StartingWith::Hash(block_hash),
                    Some(block_height) => StartingWith::BlockIdentifier(block_hash, block_height),
                }
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height) => {
                StartingWith::SyncedBlockIdentifier(block_hash, block_height)
            }
        };
        debug!("CatchUp: starting with {:?}", starting_with);

        // the block accumulator should be receiving blocks via gossiping
        // and usually has some awareness of the chain ahead of our tip
        let sync_instruction = self.block_accumulator.sync_instruction(starting_with);
        debug!("CatchUp: sync_instruction: {:?}", sync_instruction);
        match sync_instruction {
            SyncInstruction::Leap { block_hash } => {
                let leap_status = self.sync_leaper.leap_status();
                trace!("CatchUp: leap_status: {:?}", leap_status);
                match leap_status {
                    ls @ LeapStatus::Inactive | ls @ LeapStatus::Failed { .. } => {
                        if let LeapStatus::Failed {
                            error,
                            block_hash: _,
                            from_peers: _,
                            in_flight: _,
                        } = ls
                        {
                            self.attempts += 1;
                            if self.attempts > self.max_attempts {
                                return CatchUpInstruction::Shutdown(format!(
                                    "CatchUp: failed leap exceeded reattempt tolerance: {}",
                                    error,
                                ));
                            }
                        }
                        let peers_to_ask = self.small_network.peers_random_vec(
                            rng,
                            self.chainspec.core_config.simultaneous_peer_requests,
                        );
                        let effects = effect_builder.immediately().event(move |_| {
                            MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                                block_hash,
                                peers_to_ask,
                            })
                        });
                        info!("CatchUp: initiating sync leap for: {}", block_hash);
                        return CatchUpInstruction::Do(Duration::from_secs(1), effects);
                    }
                    LeapStatus::Awaiting { .. } => {
                        return CatchUpInstruction::CheckLater(
                            "sync leaper is awaiting response".to_string(),
                            Duration::from_secs(2),
                        );
                    }
                    LeapStatus::Received {
                        best_available,
                        from_peers,
                        ..
                    } => {
                        debug!("CatchUp: sync leap received: {:?}", best_available);
                        info!(
                            "CatchUp: sync leap received for: {:?}",
                            best_available.trusted_block_header.block_hash()
                        );
                        for validator_weights in best_available.era_validator_weights(
                            self.validator_matrix.fault_tolerance_threshold(),
                        ) {
                            self.validator_matrix
                                .register_era_validator_weights(validator_weights);
                        }

                        self.block_synchronizer.register_sync_leap(
                            &*best_available,
                            from_peers,
                            true,
                            self.chainspec.core_config.simultaneous_peer_requests,
                        );
                        self.block_accumulator.handle_validators(effect_builder);
                        let effects = effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        });
                        return CatchUpInstruction::Do(Duration::ZERO, effects);
                    }
                }
            }
            SyncInstruction::BlockSync { block_hash } => {
                self.block_synchronizer.register_block_by_hash(
                    block_hash,
                    true,
                    true,
                    self.chainspec.core_config.simultaneous_peer_requests,
                );

                // once started NeedNext should perpetuate until nothing is needed
                let mut effects = Effects::new();
                effects.extend(effect_builder.immediately().event(|_| {
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                }));
                return CatchUpInstruction::Do(Duration::ZERO, effects);
            }
            SyncInstruction::BlockExec { .. } => {
                let msg = "BlockExec is not valid while in CatchUp mode".to_string();
                error!("{}", msg);
                return CatchUpInstruction::Shutdown(msg);
            }
            SyncInstruction::CaughtUp => {
                if let Err(msg) = self.check_should_shutdown_for_upgrade() {
                    return CatchUpInstruction::Shutdown(msg);
                }
            }
        }
        // there are no catch up or shutdown instructions, so we must be caught up
        CatchUpInstruction::CaughtUp
    }

    fn keep_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> KeepUpInstruction {
        match self.check_should_validate(effect_builder, rng) {
            (true, Some(effects)) => {
                info!("KeepUp: go to Validate");
                return KeepUpInstruction::Validate(effects);
            }
            (_, Some(effects)) => {
                // shutting down per consensus
                return KeepUpInstruction::Do(Duration::ZERO, effects);
            }
            (_, None) => {
                // remain in KeepUp
            }
        }

        let starting_with = match self.block_synchronizer.keep_up_progress() {
            BlockSynchronizerProgress::Idle => match self.storage.read_highest_complete_block() {
                Ok(Some(block)) => StartingWith::LocalTip(block.id(), block.height()),
                Ok(None) => {
                    error!("KeepUp: block synchronizer idle, local storage has no complete blocks");
                    return KeepUpInstruction::CatchUp;
                }
                Err(err) => {
                    return KeepUpInstruction::Do(
                        Duration::ZERO,
                        utils::new_shutdown_effect(format!(
                            "fatal storage error read_highest_complete_block: {}",
                            err
                        )),
                    )
                }
            },
            BlockSynchronizerProgress::Syncing(block_hash, block_height, last_progress) => {
                // do idleness / reattempt checking
                if Timestamp::now().saturating_diff(last_progress) > self.idle_tolerance {
                    self.attempts += 1;
                } else {
                    // if any progress has been made, reset attempts
                    if last_progress > self.last_progress {
                        debug!("KeepUp: syncing last_progress: {}", last_progress);
                        self.last_progress = last_progress;
                        self.attempts = 0;
                    }
                }
                match block_height {
                    None => StartingWith::Hash(block_hash),
                    Some(height) => StartingWith::BlockIdentifier(block_hash, height),
                }
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height) => {
                debug!("KeepUp: executable block: {}", block_hash);
                StartingWith::ExecutableBlock(block_hash, block_height)
            }
        };

        debug!("KeepUp: starting with {:?}", starting_with);
        let sync_instruction = self.block_accumulator.sync_instruction(starting_with);
        debug!("KeepUp: sync_instruction {:?}", sync_instruction);
        match sync_instruction {
            SyncInstruction::Leap { .. } => KeepUpInstruction::CatchUp,
            SyncInstruction::BlockSync { block_hash } => {
                debug!("KeepUp: BlockSync: {:?}", block_hash);
                self.block_synchronizer.register_block_by_hash(
                    block_hash,
                    false,
                    true,
                    self.chainspec.core_config.simultaneous_peer_requests,
                );
                let effects = effect_builder.immediately().event(|_| {
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                });
                KeepUpInstruction::Do(Duration::ZERO, effects)
            }
            SyncInstruction::BlockExec {
                block_hash,
                next_block_hash,
            } => {
                debug!("KeepUp: BlockExec: {:?}", block_hash);
                match self.enqueue_executable_block(effect_builder, block_hash) {
                    Ok(effects) => {
                        if let Some(sync_block_hash) = next_block_hash {
                            debug!("KeepUp: BlockSync: {:?}", sync_block_hash);
                            self.block_synchronizer.register_block_by_hash(
                                sync_block_hash,
                                false,
                                true,
                                self.chainspec.core_config.simultaneous_peer_requests,
                            );
                        }
                        if effects.is_empty() {
                            KeepUpInstruction::CheckLater(
                                "KeepUp is keeping up".to_string(),
                                Duration::from_secs(1),
                            )
                        } else {
                            KeepUpInstruction::Do(Duration::ZERO, effects)
                        }
                    }
                    Err(msg) => {
                        KeepUpInstruction::Do(Duration::ZERO, utils::new_shutdown_effect(msg))
                    }
                }
            }
            SyncInstruction::CaughtUp => KeepUpInstruction::CheckLater(
                "KeepUp: at perceived tip of chain".to_string(),
                Duration::from_secs(1),
            ),
        }
    }

    fn validate_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> ValidateInstruction {
        if self.switch_block.is_none() {
            // validate status is only checked at switch blocks
            return ValidateInstruction::NonSwitchBlock;
        }
        match self.create_required_eras(effect_builder, rng) {
            Ok(Some(effects)) => {
                let last_progress = self.consensus.last_progress();
                if last_progress > self.last_progress {
                    self.last_progress = last_progress;
                }
                if effects.is_empty() {
                    ValidateInstruction::CheckLater(
                        "consensus state is up to date".to_string(),
                        Duration::from_secs(2),
                    )
                } else {
                    ValidateInstruction::Do(Duration::ZERO, effects)
                }
            }
            Ok(None) => ValidateInstruction::KeepUp,
            Err(msg) => {
                return ValidateInstruction::Do(Duration::ZERO, utils::new_shutdown_effect(msg));
            }
        }
    }

    fn check_should_validate(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> (bool, Option<Effects<MainEvent>>) {
        // if on a switch block, check if we should go to Validate
        if self.switch_block.is_some() {
            match self.create_required_eras(effect_builder, rng) {
                Err(msg) => {
                    return (false, Some(utils::new_shutdown_effect(msg)));
                }
                Ok(Some(effects)) => {
                    return (true, Some(effects));
                }
                Ok(None) => {
                    if let Err(msg) = self.check_should_shutdown_for_upgrade() {
                        return (false, Some(utils::new_shutdown_effect(msg)));
                    }
                }
            }
        }
        (false, None)
    }

    fn create_required_eras(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Result<Option<Effects<MainEvent>>, String> {
        let highest_switch_block_header = match self.recent_switch_block_headers.last() {
            None => {
                debug!("create_required_eras: recent_switch_block_headers is empty");
                return Ok(None);
            }
            Some(header) => header,
        };
        debug!(
            "highest_switch_block_header: {} - {}",
            highest_switch_block_header.era_id(),
            highest_switch_block_header.block_hash(),
        );

        if let Some(current_era) = self.consensus.current_era() {
            debug!("consensus current_era: {}", current_era.value());
            if highest_switch_block_header.next_block_era_id() <= current_era {
                return Ok(Some(Effects::new()));
            }
        }

        let highest_era_weights = match highest_switch_block_header.next_era_validator_weights() {
            None => {
                return Err(format!(
                    "highest switch block has no era end: {}",
                    highest_switch_block_header
                ));
            }
            Some(weights) => weights,
        };
        if !highest_era_weights.contains_key(self.consensus.public_key()) {
            debug!("highest_era_weights does not contain signing_public_key");
            return Ok(None);
        }

        if !self
            .deploy_buffer
            .have_full_ttl_of_deploys(highest_switch_block_header.height())
        {
            info!("currently have insufficient deploy TTL awareness to safely participate in consensus");
            return Ok(None);
        }

        let era_id = highest_switch_block_header.era_id();
        if self.upgrade_watcher.should_upgrade_after(era_id) {
            info!("upgrade required after: {}", era_id);
            return Ok(None);
        }

        let create_required_eras = self.consensus.create_required_eras(
            effect_builder,
            rng,
            &self.recent_switch_block_headers,
        );
        if create_required_eras.is_some() {
            info!("will attempt to create required eras for consensus");
        }

        Ok(
            create_required_eras
                .map(|effects| reactor::wrap_effects(MainEvent::Consensus, effects)),
        )
    }

    fn commit_genesis(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Result<Effects<MainEvent>, String> {
        match self.contract_runtime.commit_genesis(
            self.chainspec.clone().as_ref(),
            self.chainspec_raw_bytes.clone().as_ref(),
        ) {
            Ok(success) => {
                let post_state_hash = success.post_state_hash;

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
                let initial_pre_state = ExecutionPreState::new(
                    next_block_height,
                    post_state_hash,
                    BlockHash::default(),
                    Digest::default(),
                );
                self.contract_runtime
                    .set_initial_state(initial_pre_state)
                    .map_err(|err| err.to_string())?;

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
            Err(err) => Err(format!("failed to commit genesis: {:?}", err)),
        }
    }

    // todo!("remove this after we wire up method again")
    #[allow(dead_code)]
    fn commit_upgrade(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        previous_block_header: Box<BlockHeader>,
    ) -> Result<Effects<MainEvent>, String> {
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
                    let initial_pre_state = ExecutionPreState::new(
                        next_block_height,
                        post_state_hash,
                        previous_block_header.block_hash(),
                        previous_block_header.accumulated_seed(),
                    );
                    self.contract_runtime
                        .set_initial_state(initial_pre_state)
                        .map_err(|err| err.to_string())?;

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
                Err(err) => Err(format!("failed to upgrade protocol: {:?}", err)),
            },
            Err(msg) => Err(msg),
        }
    }

    fn check_should_shutdown_for_upgrade(&mut self) -> Result<(), String> {
        let highest_switch_block_era = match self.recent_switch_block_headers.last() {
            None => return Ok(()),
            Some(block_header) => block_header.era_id(),
        };
        if self
            .upgrade_watcher
            .should_upgrade_after(highest_switch_block_era)
        {
            match self
                .validator_matrix
                .validator_weights(highest_switch_block_era)
            {
                Some(validator_weights) => {
                    if self
                        .storage
                        .era_has_sufficient_finality_signatures(&validator_weights)
                    {
                        self.state = ReactorState::Upgrade;
                    }
                }
                None => {
                    return Err(
                        "should not be possible to be in this state with no era validators"
                            .to_string(),
                    );
                }
            }
        }
        Ok(())
    }

    fn enqueue_executable_block(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        block_hash: BlockHash,
    ) -> Result<Effects<MainEvent>, String> {
        let mut effects = Effects::new();
        match self.storage.make_executable_block(&block_hash) {
            Ok(Some((finalized_block, deploys))) => {
                debug!("KeepUp: enqueue_executable_block: {:?}", block_hash);
                effects.extend(
                    effect_builder
                        .enqueue_block_for_execution(finalized_block, deploys)
                        .ignore(),
                );
            }
            Ok(None) => {
                // noop
                warn!(
                    "KeepUp: idempotent enqueue_executable_block: {:?}",
                    block_hash
                );
            }
            Err(err) => {
                return Err(format!(
                    "failure to make block {} and approvals hashes into executable block: {}",
                    block_hash, err
                ));
            }
        }
        Ok(effects)
    }
}
