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
        diagnostics_port, event_stream_server, network, rest_server, rpc_server, sync_leaper,
        sync_leaper::LeapStatus,
        upgrade_watcher, InitializedComponent, ValidatorBoundComponent,
    },
    effect::{
        announcements::ControlAnnouncement, requests::BlockSynchronizerRequest, EffectBuilder,
        EffectExt, Effects,
    },
    fatal,
    reactor::{
        self,
        main_reactor::{
            catch_up_instruction::CatchUpInstruction, keep_up_instruction::KeepUpInstruction,
            upgrade_shutdown_instruction::UpgradeShutdownInstruction,
            upgrading_instruction::UpgradingInstruction, utils,
            validate_instruction::ValidateInstruction, MainEvent, MainReactor, ReactorState,
        },
    },
    types::{ActivationPoint, BlockHash, BlockPayload, FinalizedBlock, Item, SyncLeapIdentifier},
    utils::DisplayIter,
    NodeRng,
};

/// Cranking delay when encountered a non-switch block when checking the validator status.
const VALIDATION_STATUS_DELAY_FOR_NON_SWITCH_BLOCK: Duration = Duration::from_secs(2);

/// Allow the runner to shut down cleanly before shutting down the reactor.
const DELAY_BEFORE_SHUTDOWN: Duration = Duration::from_secs(2);

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
                    info!("Initialize: switch to CatchUp");
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
                    info!("CatchUp: switch to KeepUp");
                    self.state = ReactorState::KeepUp;
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
            ReactorState::KeepUp => {
                match self.keep_up_instruction(effect_builder, rng) {
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
                }
            }
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

    fn catch_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> CatchUpInstruction {
        let catch_up_progress = self.block_synchronizer.historical_progress();
        self.update_last_progress(&catch_up_progress, "CatchUp");
        let starting_with = match catch_up_progress {
            BlockSynchronizerProgress::Idle => {
                match self.trusted_hash {
                    None => {
                        // no trusted hash provided use local tip if available
                        match self.storage.read_highest_complete_block() {
                            Ok(Some(block)) => {
                                // -+ : leap w/ local tip
                                info!("CatchUp: local tip detected, no trusted hash");
                                if block.header().is_switch_block() {
                                    self.switch_block = Some(block.header().clone());
                                }
                                StartingWith::LocalTip(
                                    *block.hash(),
                                    block.height(),
                                    block.header().era_id(),
                                )
                            }
                            Ok(None) if self.switch_block.is_none() => {
                                if let Some(timestamp) = self.is_genesis() {
                                    let is_validator = self.should_commit_genesis();
                                    if false == is_validator {
                                        return CatchUpInstruction::Fatal(
                                            "CatchUp: only validating nodes may participate in genesis; cannot proceed without trusted hash".to_string(),
                                        );
                                    }
                                    let now = Timestamp::now();
                                    let grace_period =
                                        timestamp.saturating_add(TimeDiff::from_seconds(180));
                                    if now > grace_period {
                                        return CatchUpInstruction::Fatal(
                                            "CatchUp: late for genesis; cannot proceed without trusted hash".to_string(),
                                        );
                                    }
                                    let time_remaining = timestamp.saturating_diff(now);
                                    if time_remaining > TimeDiff::default() {
                                        return CatchUpInstruction::CheckLater(
                                            format!(
                                                "CatchUp: waiting for genesis activation at {}",
                                                timestamp
                                            ),
                                            Duration::from(time_remaining),
                                        );
                                    }
                                    return CatchUpInstruction::CommitGenesis;
                                }
                                // -- : no trusted hash, no local block, not genesis
                                return CatchUpInstruction::Fatal(
                                    "CatchUp: cannot proceed without trusted hash".to_string(),
                                );
                            }
                            Ok(None) => {
                                debug!("CatchUp: waiting to store genesis immediate switch block");
                                return CatchUpInstruction::CheckLater(
                                    "CatchUp: waiting for genesis immediate switch block to be stored"
                                        .to_string(),
                                    self.control_logic_default_delay.into()
                                );
                            }
                            Err(err) => {
                                return CatchUpInstruction::Fatal(format!(
                                    "CatchUp: fatal block store error when attempting to read \
                                    highest complete block: {}",
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
                                            StartingWith::LocalTip(
                                                *block.hash(),
                                                block.height(),
                                                block.header().era_id(),
                                            )
                                        }
                                    }
                                    Ok(None) => {
                                        // should be unreachable if we've gotten this far
                                        StartingWith::Hash(trusted_hash)
                                    }
                                    Err(_) => {
                                        return CatchUpInstruction::Fatal(
                                            "CatchUp: fatal block store error when attempting to \
                                            read highest complete block"
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
                                return CatchUpInstruction::Fatal(format!(
                                    "CatchUp: fatal block store error when attempting to read \
                                    highest complete block: {}",
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
                        return CatchUpInstruction::Fatal(
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
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                StartingWith::SyncedBlockIdentifier(block_hash, block_height, era_id)
            }
        };
        debug!("CatchUp: starting with {:?}", starting_with);

        // the block accumulator should be receiving blocks via gossiping
        // and usually has some awareness of the chain ahead of our tip
        let sync_instruction = self.block_accumulator.sync_instruction(starting_with);
        debug!(
            ?sync_instruction,
            "CatchUp: sync_instruction {}",
            sync_instruction.block_hash()
        );
        match sync_instruction {
            SyncInstruction::Leap { block_hash } => {
                let leap_status = self.sync_leaper.leap_status();
                trace!("CatchUp: leap_status: {:?}", leap_status);
                return match leap_status {
                    ls @ LeapStatus::Inactive | ls @ LeapStatus::Failed { .. } => {
                        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(block_hash);
                        if let LeapStatus::Failed {
                            error,
                            sync_leap_identifier: _,
                            from_peers: _,
                            in_flight: _,
                        } = ls
                        {
                            self.attempts += 1;
                            if self.attempts > self.max_attempts {
                                return CatchUpInstruction::Fatal(format!(
                                    "CatchUp: failed leap exceeded reattempt tolerance: {}",
                                    error,
                                ));
                            }
                        }
                        let peers_to_ask = self.net.fully_connected_peers_random(
                            rng,
                            self.chainspec.core_config.simultaneous_peer_requests as usize,
                        );
                        info!(
                            "CatchUp: initiating sync leap for {} using peers {}",
                            block_hash,
                            DisplayIter::new(&peers_to_ask)
                        );

                        let effects = effect_builder.immediately().event(move |_| {
                            MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                                sync_leap_identifier,
                                peers_to_ask,
                            })
                        });
                        CatchUpInstruction::Do(self.control_logic_default_delay.into(), effects)
                    }
                    LeapStatus::Awaiting { .. } => CatchUpInstruction::CheckLater(
                        "sync leaper is awaiting response".to_string(),
                        self.control_logic_default_delay.into(),
                    ),
                    LeapStatus::Received {
                        best_available,
                        from_peers,
                        ..
                    } => {
                        debug!(
                            "CatchUp: {} received from {}",
                            best_available,
                            DisplayIter::new(&from_peers)
                        );
                        info!("CatchUp: {}", best_available);
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
                        CatchUpInstruction::Do(self.control_logic_default_delay.into(), effects)
                    }
                };
            }
            SyncInstruction::BlockSync { block_hash } => {
                if self.block_synchronizer.register_block_by_hash(
                    block_hash,
                    true,
                    true,
                    self.chainspec.core_config.simultaneous_peer_requests,
                ) {
                    // once started NeedNext should perpetuate until nothing is needed
                    let mut effects = Effects::new();
                    effects.extend(effect_builder.immediately().event(|_| {
                        MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                    }));
                    return CatchUpInstruction::Do(Duration::ZERO, effects);
                }
                return CatchUpInstruction::CheckLater(
                    format!("syncing {:?}", catch_up_progress),
                    Duration::from_millis(500),
                );
            }
            SyncInstruction::CaughtUp { .. } => {
                match self.should_commit_upgrade() {
                    Ok(true) => return CatchUpInstruction::CommitUpgrade,
                    Ok(false) => (),
                    Err(msg) => return CatchUpInstruction::Fatal(msg),
                }
                if self.should_shutdown_for_upgrade() {
                    return CatchUpInstruction::ShutdownForUpgrade;
                }
            }
        }
        self.block_synchronizer.purge();
        // there are no catch up or shutdown instructions, so we must be caught up
        CatchUpInstruction::CaughtUp
    }

    fn upgrading_instruction(&self) -> UpgradingInstruction {
        match self.should_commit_upgrade() {
            Ok(true) => UpgradingInstruction::CheckLater(
                "awaiting upgrade".to_string(),
                self.control_logic_default_delay.into(),
            ),
            Ok(false) => UpgradingInstruction::CatchUp,
            Err(msg) => UpgradingInstruction::Fatal(msg),
        }
    }

    fn upgrade_shutdown_instruction(
        &self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> UpgradeShutdownInstruction {
        let highest_switch_block_era = match self.recent_switch_block_headers.last() {
            None => {
                return UpgradeShutdownInstruction::Fatal(
                    "recent_switch_block_headers cannot be empty".to_string(),
                );
            }
            Some(block_header) => block_header.era_id(),
        };
        match self
            .validator_matrix
            .validator_weights(highest_switch_block_era)
        {
            Some(validator_weights) => {
                let era_has_sufficient_finality = match self
                    .storage
                    .era_has_sufficient_finality_signatures(&validator_weights)
                {
                    Ok(is_sufficient) => is_sufficient,
                    Err(error) => {
                        return UpgradeShutdownInstruction::Fatal(format!(
                            "failed check for sufficient finality signatures: {}",
                            error
                        ));
                    }
                };
                if era_has_sufficient_finality {
                    // Allow a delay to acquire more finality signatures
                    let effects = effect_builder
                        .set_timeout(DELAY_BEFORE_SHUTDOWN)
                        .event(|_| {
                            MainEvent::ControlAnnouncement(ControlAnnouncement::ShutdownForUpgrade)
                        });
                    // should not need to crank the control logic again as the reactor will shutdown
                    UpgradeShutdownInstruction::Do(DELAY_BEFORE_SHUTDOWN, effects)
                } else {
                    UpgradeShutdownInstruction::CheckLater(
                        "waiting for sufficient finality".to_string(),
                        DELAY_BEFORE_SHUTDOWN,
                    )
                }
            }
            None => {
                UpgradeShutdownInstruction::Fatal("validator_weights cannot be missing".to_string())
            }
        }
    }

    fn keep_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> KeepUpInstruction {
        if self.should_shutdown_for_upgrade() {
            return KeepUpInstruction::ShutdownForUpgrade;
        }

        match self.should_validate(effect_builder, rng) {
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
        let forward_progress = self.block_synchronizer.forward_progress();
        self.update_last_progress(&forward_progress, "KeepUp");
        let starting_with = match forward_progress {
            BlockSynchronizerProgress::Idle => match self.storage.read_highest_complete_block() {
                Ok(Some(block)) => {
                    let block_height = block.height();
                    let state_root_hash = block.state_root_hash();
                    let block_hash = block.id();
                    let accumulated_seed = block.header().accumulated_seed();
                    match self.refresh_contract_runtime(
                        block_height + 1,
                        *state_root_hash,
                        block_hash,
                        accumulated_seed,
                    ) {
                        Ok(_) => StartingWith::LocalTip(
                            block.id(),
                            block_height,
                            block.header().era_id(),
                        ),
                        Err(msg) => {
                            return KeepUpInstruction::Fatal(msg);
                        }
                    }
                }
                Ok(None) => {
                    error!("KeepUp: block synchronizer idle, local storage has no complete blocks");
                    self.block_synchronizer.purge();
                    return KeepUpInstruction::CatchUp;
                }
                Err(error) => {
                    return KeepUpInstruction::Fatal(format!(
                        "failed to read highest complete block: {}",
                        error
                    ))
                }
            },
            BlockSynchronizerProgress::Syncing(block_hash, block_height, _) => match block_height {
                None => StartingWith::Hash(block_hash),
                Some(height) => StartingWith::BlockIdentifier(block_hash, height),
            },
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                debug!("KeepUp: synced block: {}", block_hash);
                self.block_synchronizer.purge_forward();
                StartingWith::SyncedBlockIdentifier(block_hash, block_height, era_id)
            }
        };
        debug!(
            ?starting_with,
            "KeepUp: starting with {}",
            starting_with.block_hash()
        );
        let sync_instruction = self.block_accumulator.sync_instruction(starting_with);
        debug!(
            ?sync_instruction,
            "KeepUp: sync_instruction {}",
            sync_instruction.block_hash()
        );
        match sync_instruction {
            SyncInstruction::Leap { .. } => {
                self.block_synchronizer.purge();
                return KeepUpInstruction::CatchUp;
            }
            SyncInstruction::BlockSync { block_hash } => {
                debug!("KeepUp: BlockSync: {:?}", block_hash);
                if self.block_synchronizer.register_block_by_hash(
                    block_hash,
                    false,
                    true,
                    self.chainspec.core_config.simultaneous_peer_requests,
                ) {
                    info!(
                        ?block_hash,
                        "KeepUp: BlockSync: registered block by hash {}", block_hash
                    );
                    return KeepUpInstruction::Do(
                        Duration::ZERO,
                        effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        }),
                    );
                }
            }
            SyncInstruction::CaughtUp { .. } => {
                // noop
            }
        }
        if false == self.sync_to_historical {
            // if nothing else needs to be done, check later
            return KeepUpInstruction::CheckLater(
                "at perceived tip of chain".to_string(),
                self.control_logic_default_delay.into(),
            );
        }
        let historical_progress = self.block_synchronizer.historical_progress();
        self.update_last_progress(&historical_progress, "Historical");
        match self.maybe_parent_block_identifier(&historical_progress) {
            Err(msg) => KeepUpInstruction::Fatal(msg),
            Ok(None) => KeepUpInstruction::CheckLater(
                format!("Historical: syncing {:?}", historical_progress),
                self.control_logic_default_delay.into(),
            ),
            Ok(Some((parent_hash, era_id))) => {
                if false == self.validator_matrix.has_era(&era_id) {
                    debug!("Historical: sync leaping for: {}", parent_hash);
                    let leap_status = self.sync_leaper.leap_status();
                    debug!("Historical: {:?}", leap_status);
                    match leap_status {
                        ls @ LeapStatus::Inactive | ls @ LeapStatus::Failed { .. } => {
                            if let LeapStatus::Failed {
                                error,
                                sync_leap_identifier: _,
                                from_peers: _,
                                in_flight: _,
                            } = ls
                            {
                                self.attempts += 1;
                                if self.attempts > self.max_attempts {
                                    // self.crank will ensure shut down if no other progress
                                    // is made before this event is processed
                                    let msg = format!(
                                        "Historical: failed leap back exceeded reattempt tolerance: {}",
                                        error,
                                    );
                                    warn!("{}", msg);
                                    return KeepUpInstruction::CheckLater(msg, Duration::ZERO);
                                }
                                error!("Historical: sync leap failed: {:?}", error);
                            }
                            let peers_to_ask = self.net.fully_connected_peers_random(
                                rng,
                                self.chainspec.core_config.simultaneous_peer_requests as usize,
                            );
                            let sync_leap_identifier =
                                SyncLeapIdentifier::sync_to_historical(parent_hash);
                            let effects = effect_builder.immediately().event(move |_| {
                                MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                                    sync_leap_identifier,
                                    peers_to_ask,
                                })
                            });
                            info!("Historical: initiating sync leap for: {}", parent_hash);
                            KeepUpInstruction::Do(Duration::ZERO, effects)
                        }
                        LeapStatus::Received {
                            best_available,
                            from_peers: _,
                            ..
                        } => {
                            info!(
                                "Historical: sync leap received for: {:?}",
                                best_available.trusted_block_header.block_hash()
                            );
                            let era_validator_weights = best_available.era_validator_weights(
                                self.validator_matrix.fault_tolerance_threshold(),
                            );
                            for evw in era_validator_weights {
                                let era_id = evw.era_id();
                                if self.validator_matrix.register_era_validator_weights(evw) {
                                    info!("Historical: got era: {}", era_id);
                                } else {
                                    debug!("Historical: already had era: {}", era_id);
                                }
                            }
                            KeepUpInstruction::CheckLater(
                                "Historical: sync leap received".to_string(),
                                Duration::ZERO,
                            )
                        }
                        LeapStatus::Awaiting { .. } => KeepUpInstruction::CheckLater(
                            "Historical: sync leap awaiting".to_string(),
                            self.control_logic_default_delay.into(),
                        ),
                    }
                } else if self.block_synchronizer.register_block_by_hash(
                    parent_hash,
                    true,
                    true,
                    self.chainspec.core_config.simultaneous_peer_requests,
                ) {
                    info!("Historical: register_block_by_hash: {}", parent_hash);
                    let peers_to_ask = self.net.fully_connected_peers_random(
                        rng,
                        self.chainspec.core_config.simultaneous_peer_requests as usize,
                    );
                    debug!("Historical: peers count: {:?}", peers_to_ask.len());
                    self.block_synchronizer
                        .register_peers(parent_hash, peers_to_ask);

                    return KeepUpInstruction::Do(
                        Duration::ZERO,
                        effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        }),
                    );
                } else {
                    self.block_synchronizer.purge_historical();
                    KeepUpInstruction::CheckLater(
                        "Historical: purged".to_string(),
                        self.control_logic_default_delay.into(),
                    )
                }
            }
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

        if self.should_shutdown_for_upgrade() {
            return ValidateInstruction::ShutdownForUpgrade;
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
                        self.control_logic_default_delay.into(),
                    )
                } else {
                    ValidateInstruction::Do(Duration::ZERO, effects)
                }
            }
            Ok(None) => ValidateInstruction::KeepUp,
            Err(msg) => ValidateInstruction::Fatal(msg),
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
            &mut self.net,
            MainEvent::Network(network::Event::Initialize),
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
            &mut self.rpc_server,
            MainEvent::RpcServer(rpc_server::Event::Initialize),
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
            let event = deploy_buffer::Event::Initialize(blocks);
            if let Some(effects) = utils::initialize_component(
                effect_builder,
                &mut self.deploy_buffer,
                MainEvent::DeployBuffer(event),
            ) {
                return Some(effects);
            }
        }
        None
    }

    fn update_last_progress(
        &mut self,
        block_synchronizer_progress: &BlockSynchronizerProgress,
        phase_prefix: &str,
    ) {
        if let BlockSynchronizerProgress::Syncing(_, _, last_progress) = block_synchronizer_progress
        {
            // do idleness / reattempt checking
            let sync_progress = *last_progress;
            if sync_progress > self.last_progress {
                self.last_progress = sync_progress;
                // if any progress has been made, reset attempts
                self.attempts = 0;
            }
            if self.last_progress.elapsed() > self.idle_tolerance {
                self.attempts += 1;
            }
        }
        debug!(
            "{}: last_progress: {} {}",
            phase_prefix, self.last_progress, block_synchronizer_progress
        );
    }

    fn maybe_parent_block_identifier(
        &mut self,
        block_synchronizer_progress: &BlockSynchronizerProgress,
    ) -> Result<Option<(BlockHash, EraId)>, String> {
        if matches!(
            block_synchronizer_progress,
            BlockSynchronizerProgress::Syncing(_, _, _)
        ) {
            return Ok(None);
        }
        if let Some(block_header) = self.storage.get_highest_orphaned_block_header() {
            if block_header.is_genesis() {
                self.block_synchronizer.purge_historical();
                return Ok(None);
            }
            debug!(
                "Historical: attempting({}) for: {}",
                block_header.height().saturating_sub(1),
                block_header.parent_hash()
            );
            match self.storage.read_block_header(block_header.parent_hash()) {
                Ok(Some(parent)) => Ok(Some((parent.block_hash(), parent.era_id()))),
                Ok(None) => match block_header.era_id().predecessor() {
                    Some(previous_era_id) => {
                        Ok(Some((*block_header.parent_hash(), previous_era_id)))
                    }
                    None => Ok(None),
                },
                Err(err) => Err(err.to_string()),
            }
        } else {
            Ok(None)
        }
    }

    fn should_validate(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> (bool, Option<Effects<MainEvent>>) {
        if self.switch_block.is_some() {
            self.check_validate_latch = None;
        }
        if let Some(should_validate) = self.check_validate_latch {
            return (should_validate, None);
        }
        match self.create_required_eras(effect_builder, rng) {
            Err(msg) => {
                return (false, Some(fatal!(effect_builder, "{}", msg).ignore()));
            }
            Ok(Some(effects)) => {
                self.check_validate_latch = Some(true);
                (true, Some(effects))
            }
            Ok(None) => {
                self.check_validate_latch = Some(false);
                (false, None)
            }
        }
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
            debug!(%era_id, "upgrade required after given era");
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

    fn is_genesis(&self) -> Option<Timestamp> {
        match self.chainspec.protocol_config.activation_point {
            ActivationPoint::Genesis(timestamp) => Some(timestamp),
            ActivationPoint::EraId(_) => None,
        }
    }

    fn should_commit_genesis(&self) -> bool {
        self.chainspec
            .network_config
            .is_genesis_validator(self.validator_matrix.public_signing_key())
            .unwrap_or(false)
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
        self.refresh_contract_runtime(
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

    fn should_commit_upgrade(&self) -> Result<bool, String> {
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
                    self.refresh_contract_runtime(
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

    fn should_shutdown_for_upgrade(&self) -> bool {
        if let Some(block_header) = self.recent_switch_block_headers.last() {
            return self
                .upgrade_watcher
                .should_upgrade_after(block_header.era_id());
        }
        false
    }

    fn refresh_contract_runtime(
        &mut self,
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) -> Result<(), String> {
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
}
