use datasize::DataSize;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, Timestamp};

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
        upgrade_watcher, InitializedComponent,
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor::{
        self,
        main_reactor::{utils, MainEvent, MainReactor},
        Reactor,
    },
    types::{
        ActivationPoint, BlockHash, BlockHeader, BlockPayload, FinalizedBlock, ValidatorMatrix,
    },
    NodeRng,
};

// todo!: put in the config
pub(crate) const WAIT_DURATION: Duration = Duration::from_secs(5);

#[derive(Copy, Clone, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum ReactorState {
    // get all components and reactor state set up on start
    Initialize,
    // orient to the network and attempt to catch up to tip
    CatchUp,
    // stay caught up with tip
    KeepUp,
    // node is currently caught up and is an active validator
    Validate,
    // node should be shut down for upgrade
    Upgrade,
}

enum CatchUpInstruction {
    Do(Effects<MainEvent>),
    CheckSoon(String),
    CheckLater(String, Duration),
    Shutdown(String),
    CaughtUp,
    CommitGenesis,
}

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
            ReactorState::Initialize => {
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.diagnostics_port,
                    "diagnostics",
                    MainEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.upgrade_watcher,
                    "upgrade_watcher",
                    MainEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.small_network,
                    "small_network",
                    MainEvent::Network(small_network::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.event_stream_server,
                    "event_stream_server",
                    MainEvent::EventStreamServer(event_stream_server::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.rest_server,
                    "rest_server",
                    MainEvent::RestServer(rest_server::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.rpc_server,
                    "rpc_server",
                    MainEvent::RpcServer(rpc_server::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if let Some(effects) = utils::initialize_component(
                    effect_builder,
                    &mut self.block_synchronizer,
                    "block_synchronizer",
                    MainEvent::BlockSynchronizer(block_synchronizer::Event::Initialize),
                ) {
                    return (Duration::ZERO, effects);
                }
                if <DeployBuffer as InitializedComponent<MainEvent>>::is_uninitialized(
                    &self.deploy_buffer,
                ) {
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
                            return (
                                Duration::ZERO,
                                utils::new_shutdown_effect(format!(
                                    "fatal block store error when attempting to read highest \
                                    blocks: {}",
                                    err
                                )),
                            )
                        }
                    };
                    let event = deploy_buffer::Event::Initialize(blocks);
                    if let Some(effects) = utils::initialize_component(
                        effect_builder,
                        &mut self.deploy_buffer,
                        "deploy_buffer",
                        MainEvent::DeployBuffer(event),
                    ) {
                        return (Duration::ZERO, effects);
                    }
                }
                // apply upgrade here?
                self.state = ReactorState::CatchUp;
                return (Duration::ZERO, Effects::new());
            }
            ReactorState::CatchUp => match self.catch_up_instructions(rng, effect_builder) {
                CatchUpInstruction::CommitGenesis => match self.commit_genesis(effect_builder) {
                    Ok(effects) => return (WAIT_DURATION, effects),
                    Err(msg) => return (Duration::ZERO, utils::new_shutdown_effect(msg)),
                },
                CatchUpInstruction::Do(effects) => return (WAIT_DURATION, effects),
                CatchUpInstruction::CheckLater(_, wait) => return (wait, Effects::new()),
                CatchUpInstruction::CheckSoon(_) => return (Duration::ZERO, Effects::new()),
                CatchUpInstruction::Shutdown(msg) => {
                    return (Duration::ZERO, utils::new_shutdown_effect(msg))
                }
                CatchUpInstruction::CaughtUp => {
                    self.state = ReactorState::KeepUp;
                    return (Duration::ZERO, Effects::new());
                }
            },
            ReactorState::KeepUp => {
                // todo! if sync to genesis == true, determine if cycles
                // are available to get next earliest historical block via synchronizer
                self.block_synchronizer.flush_dishonest_peers();

                let starting_with =
                    match self.block_synchronizer.maybe_executable_block_identifier() {
                        Some((block_hash, block_height)) => {
                            StartingWith::ExecutableBlock(block_hash, block_height)
                        }
                        None => match self.block_synchronizer.maybe_finished_block_identifier() {
                            None => {
                                // this should be unreachable
                                self.state = ReactorState::CatchUp;
                                return (Duration::ZERO, Effects::new());
                            }
                            Some((block_hash, block_height)) => {
                                StartingWith::BlockIdentifier(block_hash, block_height)
                            }
                        },
                    };

                let instruction = self.block_accumulator.sync_instruction(starting_with);

                match instruction {
                    SyncInstruction::Leap => {
                        // we've fallen behind, go back to catch up mode
                        self.state = ReactorState::CatchUp;
                    }
                    SyncInstruction::BlockSync {
                        block_hash,
                        should_fetch_execution_state,
                    } => {
                        self.block_synchronizer.register_block_by_hash(
                            block_hash,
                            should_fetch_execution_state,
                            true,
                            self.chainspec
                                .core_config
                                .sync_leap_simultaneous_peer_requests,
                        );
                        let effects = effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        });
                        return (WAIT_DURATION, effects);
                    }
                    SyncInstruction::CaughtUp => {
                        match self.create_required_eras(effect_builder, rng) {
                            // if node is in validator set and era supervisor has what it needs
                            // to run, switch to validate mode
                            Ok(Some(mut effects)) => {
                                self.state = ReactorState::Validate;
                                self.block_synchronizer.turn_off();
                                return (Duration::ZERO, effects);
                            }
                            Ok(None) => {
                                if let Err(msg) = self.check_upgrade() {
                                    return (Duration::ZERO, utils::new_shutdown_effect(msg));
                                }
                            }
                            Err(msg) => return (Duration::ZERO, utils::new_shutdown_effect(msg)),
                        }
                    }
                    SyncInstruction::BlockExec { next_block_hash } => {
                        match self.enqueue_executable_block(effect_builder) {
                            Ok(effects) => {
                                if let Some(block_hash) = next_block_hash {
                                    self.block_synchronizer.register_block_by_hash(
                                        block_hash,
                                        false,
                                        true,
                                        self.chainspec
                                            .core_config
                                            .sync_leap_simultaneous_peer_requests,
                                    );
                                }
                                return (Duration::ZERO, effects);
                            }
                            Err(msg) => return (Duration::ZERO, utils::new_shutdown_effect(msg)),
                        }
                    }
                }
            }
            ReactorState::Validate => {
                //if self.upgrade_watcher.should_upgrade_after(era_id)
                // todo! only validators should actually apply upgrades and sign them and gossip
                // them; other nodes should instead sync to them
                //     match self.commit_upgrade(effect_builder, block_header) {
                //         Ok(effects) => return (WAIT_DURATION, effects),
                //         Err(msg) => return (Duration::ZERO, utils::new_shutdown_effect(msg)),
                //     }
                match self.create_required_eras(effect_builder, rng) {
                    Ok(Some(effects)) => return (Duration::ZERO, effects),
                    Ok(None) => {
                        // either consensus doesn't have enough protocol data
                        // or this node has been evicted or has naturally
                        // fallen out of the validator set in a new era.
                        // regardless, go back to keep up mode;
                        // the keep up logic will handle putting them back
                        // to catch up if necessary, or back to validate
                        // if they become able to validate again
                        self.state = ReactorState::KeepUp;
                        self.block_synchronizer.turn_on();
                    }
                    Err(msg) => return (Duration::ZERO, utils::new_shutdown_effect(msg)),
                }
                let last_progress = self.consensus.last_progress();
                if last_progress > self.last_progress {
                    self.last_progress = last_progress;
                }
            }
            ReactorState::Upgrade => {
                // noop ... patiently wait for the runner to shut down cleanly
            }
        }
        (Duration::from_secs(1), Effects::new())
    }

    fn catch_up_instructions(
        &mut self,
        rng: &mut NodeRng,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> CatchUpInstruction {
        let starting_with = match self.block_synchronizer.catch_up_progress() {
            BlockSynchronizerProgress::Idle => {
                match self.trusted_hash {
                    None => {
                        match self.storage.read_highest_complete_block() {
                            // no trusted hash provided use local tip if available
                            Ok(Some(block)) => {
                                // -+ : leap w/ local tip
                                StartingWith::BlockIdentifier(*block.hash(), block.height())
                            }
                            Ok(None) => {
                                if let ActivationPoint::Genesis(timestamp) =
                                    self.chainspec.protocol_config.activation_point
                                {
                                    if Timestamp::now() <= timestamp {
                                        // the network is in pre-genesis
                                        return CatchUpInstruction::CommitGenesis;
                                    }
                                }
                                // we are post genesis, have no local blocks, and no trusted hash
                                // so we can't possibly catch up to the network and should shut down
                                return CatchUpInstruction::Shutdown(
                                    "post-genesis; cannot proceed without trusted hash provided"
                                        .to_string(),
                                );
                            }
                            Err(err) => {
                                return CatchUpInstruction::Shutdown(format!(
                                    "fatal block store error when attempting to read highest \
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
                                            "fatal block store error when attempting to read highest \
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
                                    "fatal block store error when attempting to read highest \
                            complete block: {}",
                                    err
                                ));
                            }
                        }
                    }
                }
            }
            BlockSynchronizerProgress::Syncing(block_hash, maybe_block_height, last_progress) => {
                // do the shutdown / reattempt checking and are we still near tip
                if Timestamp::now().saturating_diff(last_progress) > self.idle_tolerances {
                    self.attempts += 1;
                    if self.attempts > self.max_attempts {
                        return CatchUpInstruction::Shutdown(
                            "catch up process exceeds idle tolerances".to_string(),
                        );
                    }
                }
                // if any progress has been made, reset attempts
                if last_progress > self.last_progress {
                    self.last_progress = last_progress;
                    self.attempts = 0;
                }
                match maybe_block_height {
                    None => StartingWith::Hash(block_hash),
                    Some(block_height) => StartingWith::BlockIdentifier(block_hash, block_height),
                }
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height) => {
                // do this dance:
                StartingWith::SyncedBlockIdentifier(block_hash, block_height)
            }
        };

        // the block accumulator should be receiving blocks via gossiping
        // and usually has some awareness of the chain ahead of our tip
        let trusted_hash = starting_with.block_hash();
        match self.block_accumulator.sync_instruction(starting_with) {
            SyncInstruction::Leap => match self.sync_leaper.leap_status() {
                LeapStatus::Inactive | LeapStatus::Failed { .. } => {
                    let peers_to_ask = self.small_network.peers_random_vec(
                        rng,
                        self.chainspec
                            .core_config
                            .sync_leap_simultaneous_peer_requests,
                    );
                    let effects = effect_builder.immediately().event(move |_| {
                        MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                            trusted_hash,
                            peers_to_ask,
                        })
                    });
                    return CatchUpInstruction::Do(effects);
                }
                LeapStatus::Awaiting { .. } => {
                    return CatchUpInstruction::CheckSoon(
                        "sync leaper is awaiting response".to_string(),
                    );
                }
                LeapStatus::Received {
                    best_available,
                    from_peers,
                    ..
                } => {
                    for validator_weights in best_available
                        .era_validator_weights(self.validator_matrix.fault_tolerance_threshold())
                    {
                        self.validator_matrix
                            .register_era_validator_weights(validator_weights);
                    }

                    // todo! maybe register sync_leap to accumulator instead
                    self.block_synchronizer.register_sync_leap(
                        &*best_available,
                        from_peers,
                        true,
                        self.chainspec
                            .core_config
                            .sync_leap_simultaneous_peer_requests,
                    );

                    self.block_accumulator
                        .register_updated_validator_matrix(effect_builder);
                    let effects = effect_builder.immediately().event(|_| {
                        MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                    });
                    return CatchUpInstruction::Do(effects);
                }
            },
            SyncInstruction::BlockSync {
                block_hash,
                should_fetch_execution_state,
            } => {
                if should_fetch_execution_state == false {
                    let msg = format!(
                        "BlockSync should require execution state while in CatchUp mode: {}",
                        block_hash
                    );
                    error!("{}", msg);
                    return CatchUpInstruction::Shutdown(msg);
                }

                self.block_synchronizer.register_block_by_hash(
                    block_hash,
                    should_fetch_execution_state,
                    true,
                    self.chainspec
                        .core_config
                        .sync_leap_simultaneous_peer_requests,
                );

                // once started NeedNext should perpetuate until nothing is needed
                let mut effects = Effects::new();
                effects.extend(effect_builder.immediately().event(|_| {
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                }));
                return CatchUpInstruction::Do(effects);
            }
            SyncInstruction::BlockExec { .. } => {
                let msg = "BlockExec is not valid while in CatchUp mode".to_string();
                error!("{}", msg);
                return CatchUpInstruction::Shutdown(msg);
            }
            SyncInstruction::CaughtUp => {
                if let Err(msg) = self.check_upgrade() {
                    return CatchUpInstruction::Shutdown(msg);
                }
            }
        }

        // there are no catch up or shutdown instructions, so we must be caught up
        CatchUpInstruction::CaughtUp
    }

    fn create_required_eras(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Result<Option<Effects<MainEvent>>, String> {
        let highest_switch_block_header = match self.recent_switch_block_headers.last() {
            None => return Ok(None),
            Some(header) => header,
        };

        if let Some(current_era) = self.consensus.current_era() {
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
            return Ok(None);
        }

        if !self
            .deploy_buffer
            .have_full_ttl_of_deploys(highest_switch_block_header.height())
        {
            return Ok(None);
        }

        if self
            .upgrade_watcher
            .should_upgrade_after(highest_switch_block_header.era_id())
        {
            return Ok(None);
        }

        Ok(self
            .consensus
            .create_required_eras(effect_builder, rng, &self.recent_switch_block_headers)
            .map(|effects| reactor::wrap_effects(MainEvent::Consensus, effects)))
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

    fn check_upgrade(&mut self) -> Result<(), String> {
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
    ) -> Result<Effects<MainEvent>, String> {
        let mut effects = Effects::new();
        if let Some((block_hash, block_height)) =
            self.block_synchronizer.maybe_executable_block_identifier()
        {
            match self.storage.make_executable_block(&block_hash) {
                Ok(Some((finalized_block, deploys))) => {
                    effects.extend(
                        effect_builder
                            .enqueue_block_for_execution(finalized_block, deploys)
                            .ignore(),
                    );
                }
                Ok(None) => {
                    // noop
                }
                Err(err) => {
                    return Err(format!(
                        "failure to make block {} and approvals hashes into executable block: {}",
                        block_hash, err
                    ));
                }
            }
        }
        Ok(effects)
    }
}
