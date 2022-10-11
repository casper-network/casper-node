use datasize::DataSize;
use log::{debug, error};
use std::{sync::Arc, time::Duration};
use tracing::{info, warn};

use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::{
        block_accumulator::{StartingWith, SyncInstruction},
        consensus::EraReport,
        contract_runtime::ExecutionPreState,
        diagnostics_port, event_stream_server,
        linear_chain::era_validator_weights_for_block,
        rest_server, rpc_server, small_network,
        storage::FatalStorageError,
        sync_leaper,
        sync_leaper::LeapStatus,
        upgrade_watcher,
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor,
    reactor::main_reactor::{
        utils::{enqueue_shutdown, initialize_component},
        MainEvent, MainReactor,
    },
    types::{
        ActivationPoint, ApprovalsHashes, BlockHash, BlockHeader, BlockPayload, ChainspecRawBytes,
        DeployHash, EraValidatorWeights, FinalizedBlock, Item, ValidatorMatrix,
    },
    NodeRng,
};

use crate::components::{block_synchronizer, deploy_buffer};
use casper_execution_engine::core::{
    engine_state,
    engine_state::{GenesisSuccess, UpgradeSuccess},
};

// put in the config
pub(crate) const WAIT_SEC: u64 = 5;

#[derive(DataSize, Debug)]
pub(crate) enum ReactorState {
    // get all components and reactor state set up on start
    Initialize,
    // orient to the network and attempt to catch up to tip
    CatchUp,
    // stay caught up with tip
    KeepUp,
    // node is currently caught up and is an active validator
    Validate,
}

enum CatchUpInstruction {
    Do(Effects<MainEvent>),
    CheckSoon(String),
    CheckLater(String, u64),
    Shutdown(String),
    CaughtUp,
    CommitGenesis,
    CommitUpgrade(Box<BlockHeader>),
}

enum KeepUpInstruction {}

impl MainReactor {
    pub(crate) fn crank(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Effects<MainEvent> {
        match self.state {
            ReactorState::Initialize => {
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.diagnostics_port,
                    "diagnostics",
                    MainEvent::DiagnosticsPort(diagnostics_port::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.upgrade_watcher,
                    "upgrade_watcher",
                    MainEvent::UpgradeWatcher(upgrade_watcher::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.small_network,
                    "small_network",
                    MainEvent::Network(small_network::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.event_stream_server,
                    "event_stream_server",
                    MainEvent::EventStreamServer(event_stream_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.rest_server,
                    "rest_server",
                    MainEvent::RestServer(rest_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.rpc_server,
                    "rpc_server",
                    MainEvent::RpcServer(rpc_server::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.block_synchronizer,
                    "block_synchronizer",
                    MainEvent::BlockSynchronizer(block_synchronizer::Event::Initialize),
                ) {
                    return effects;
                }
                if let Some(effects) = initialize_component(
                    effect_builder,
                    &mut self.deploy_buffer,
                    "deploy_buffer",
                    MainEvent::DeployBuffer(deploy_buffer::Event::Initialize),
                ) {
                    return effects;
                }
                self.state = ReactorState::CatchUp;
                return effect_builder
                    .immediately()
                    .event(|_| MainEvent::ReactorCrank);
            }
            ReactorState::CatchUp => match self.catch_up_instructions(rng, effect_builder) {
                CatchUpInstruction::CommitGenesis => {
                    let mut ret = Effects::new();
                    match self.commit_genesis(effect_builder) {
                        Ok(effects) => {
                            ret.extend(effects);
                            ret.extend(
                                effect_builder
                                    .set_timeout(Duration::from_secs(WAIT_SEC))
                                    .event(|_| MainEvent::ReactorCrank),
                            );
                        }
                        Err(msg) => {
                            ret.extend(
                                effect_builder
                                    .immediately()
                                    .event(move |_| MainEvent::Shutdown(msg)),
                            );
                        }
                    }
                    return ret;
                }
                CatchUpInstruction::CommitUpgrade(block_header) => {
                    let mut ret = Effects::new();
                    match self.commit_upgrade(effect_builder, block_header) {
                        Ok(effects) => {
                            ret.extend(effects);
                            ret.extend(
                                effect_builder
                                    .set_timeout(Duration::from_secs(WAIT_SEC))
                                    .event(|_| MainEvent::ReactorCrank),
                            );
                        }
                        Err(msg) => {
                            ret.extend(
                                effect_builder
                                    .immediately()
                                    .event(move |_| MainEvent::Shutdown(msg)),
                            );
                        }
                    }
                    return ret;
                }
                CatchUpInstruction::Do(effects) => {
                    let mut ret = Effects::new();
                    ret.extend(effects);
                    ret.extend(
                        effect_builder
                            .set_timeout(Duration::from_secs(WAIT_SEC))
                            .event(|_| MainEvent::ReactorCrank),
                    );
                    return ret;
                }
                CatchUpInstruction::CheckLater(_, wait) => {
                    return effect_builder
                        .set_timeout(Duration::from_secs(wait))
                        .event(|_| MainEvent::ReactorCrank);
                }
                CatchUpInstruction::CheckSoon(_) => {
                    return effect_builder
                        .immediately()
                        .event(|_| MainEvent::ReactorCrank);
                }
                CatchUpInstruction::Shutdown(msg) => {
                    return effect_builder
                        .immediately()
                        .event(move |_| MainEvent::Shutdown(msg))
                }
                CatchUpInstruction::CaughtUp => {
                    self.state = ReactorState::KeepUp;
                    return effect_builder
                        .immediately()
                        .event(|_| MainEvent::ReactorCrank);
                }
            },
            ReactorState::KeepUp => {
                // TODO: if UpgradeWatcher announcement raised, keep track of era id's against
                // the new activation points detected upgrade to make this a stronger check

                // TODO: if sync to genesis == true, determine if cycles
                // are available and if so, queue up block sync to get next
                // missing historical block

                // TODO: double check getting this from block_sync actually makes sense
                let starting_with = match self.block_synchronizer.maybe_executable_block_hash() {
                    // TODO: We might have the complete block, with state: We should execute.
                    // If we don't, we might want to go back to CatchUp mode?
                    Some(block_hash) => StartingWith::Hash(block_hash),
                    // TODO: We might just not have finalized the next block yet. No need to leap
                    // again!
                    None => StartingWith::Nothing,
                };

                match self.block_accumulator.sync_instruction(starting_with) {
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
                        let mut effects = Effects::new();
                        effects.extend(effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        }));
                        effects.extend(
                            effect_builder
                                .set_timeout(Duration::from_secs(WAIT_SEC))
                                .event(|_| MainEvent::ReactorCrank),
                        );
                        return effects;
                    }
                    SyncInstruction::CaughtUp => {
                        // if node is in validator set and era supervisor has what it needs
                        // to run, switch to validate mode
                        match self.should_we_validate(effect_builder, rng) {
                            Ok(Some(mut effects)) => {
                                self.state = ReactorState::Validate;
                                self.block_synchronizer.turn_off();
                                effects.extend(
                                    effect_builder
                                        .immediately()
                                        .event(|_| MainEvent::ReactorCrank),
                                );
                                return effects;
                            }
                            Ok(None) => {}
                            Err(error_msg) => {
                                return effect_builder
                                    .immediately()
                                    .event(move |_| MainEvent::Shutdown(error_msg))
                            }
                        }
                        match self.enqueue_executable_block(effect_builder) {
                            Ok(mut effects) => {
                                effects.extend(
                                    effect_builder
                                        .immediately()
                                        .event(|_| MainEvent::ReactorCrank),
                                );
                                return effects;
                            }
                            Err(msg) => {
                                return effect_builder
                                    .immediately()
                                    .event(move |_| MainEvent::Shutdown(msg));
                            }
                        }
                    }
                }
            }
            ReactorState::Validate => {
                // todo!
                // if self.block_accumulator.foo() {
                // we have a block with sufficient finality sigs to start gossiping
                // enqueue it (add event or direct gossiper call)
                // }
                match self.should_we_validate(effect_builder, rng) {
                    Ok(Some(mut effects)) => {
                        effects.extend(
                            effect_builder
                                .immediately()
                                .event(|_| MainEvent::ReactorCrank),
                        );
                        return effects;
                    }
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
                    Err(error_msg) => {
                        return effect_builder
                            .immediately()
                            .event(move |_| MainEvent::Shutdown(error_msg))
                    }
                }

                if self.consensus.last_progress().elapsed() <= self.idle_tolerances {
                    // TODO: Reset counter?
                    // TODO: ELSE: Increment counter, leap?
                    // With Highway we must make sure that the whole network doesn't restart
                    // at the same time, though, since it doesn't restore protocol state.
                }
            }
        }
        // TODO: Really immediately?
        effect_builder
            .immediately()
            .event(|_| MainEvent::ReactorCrank)
    }

    fn catch_up_instructions(
        &mut self,
        rng: &mut NodeRng,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> CatchUpInstruction {
        let mut effects = Effects::new();
        // check idleness & enforce re-attempts if necessary
        if let Some(last_progress) = self.block_synchronizer.last_progress() {
            if Timestamp::now().saturating_diff(last_progress) <= self.idle_tolerances {
                self.attempts = 0; // if any progress has been made, reset attempts
                let mut effects = Effects::new();
                effects.extend(effect_builder.immediately().event(|_| {
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                }));
                return CatchUpInstruction::Do(effects);
            }
            self.attempts += 1;
            if self.attempts > self.max_attempts {
                return CatchUpInstruction::Shutdown(
                    "catch up process exceeds idle tolerances".to_string(),
                );
            }
        }

        // determine which block / block_hash we should attempt to leap from
        let starting_with = match self.trusted_hash {
            None => {
                match self.linear_chain.highest_block() {
                    // no trusted hash provided use local tip if available
                    Some(block) => {
                        // -+ : leap w/ local tip
                        StartingWith::Block(Box::new(block.clone()))
                    }
                    None => {
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
                }
            }
            Some(trusted_hash) => {
                match self.storage.read_block(&trusted_hash) {
                    Ok(Some(trusted_block)) => {
                        match self.linear_chain.highest_block() {
                            Some(block) => {
                                // ++ : leap w/ the higher of local tip or trusted hash
                                if trusted_block.height() > block.height() {
                                    StartingWith::Hash(trusted_hash)
                                } else {
                                    StartingWith::Block(Box::new(block.clone()))
                                }
                            }
                            None => {
                                // should be unreachable if we've gotten this far
                                StartingWith::Hash(trusted_hash)
                            }
                        }
                    }
                    Ok(None) => {
                        // +- : leap w/ config hash
                        StartingWith::Hash(trusted_hash)
                    }
                    Err(_) => {
                        return CatchUpInstruction::Shutdown(
                                "fatal block store error when attempting to read block under trusted hash".to_string(),
                            );
                    }
                }
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
                    effects.extend(effect_builder.immediately().event(move |_| {
                        MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                            trusted_hash,
                            peers_to_ask,
                        })
                    }));
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
                    let era_id = best_available.highest_era();
                    let validator_weights = match best_available.validators_of_highest_block() {
                        Some(validator_weights) => validator_weights,
                        None => {
                            let msg = format!(
                                "unable to read validators_of_highest_block from {:?}",
                                best_available
                            );
                            error!("{}", msg);
                            return CatchUpInstruction::Shutdown(msg);
                        }
                    };
                    self.block_synchronizer.register_sync_leap(
                        &*best_available,
                        from_peers,
                        true,
                        self.chainspec
                            .core_config
                            .sync_leap_simultaneous_peer_requests,
                    );

                    let mut validator_matrix = ValidatorMatrix::new(
                        self.chainspec.highway_config.finality_threshold_fraction,
                    );
                    validator_matrix.register_validator_weights(era_id, validator_weights.clone());
                    self.block_accumulator
                        .register_updated_validator_matrix(validator_matrix);
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
            SyncInstruction::CaughtUp => {
                // TODO: should we be using linear chain for this?
                match self.linear_chain.highest_block() {
                    Some(block) => {
                        let era_id = block.header().era_id();
                        // TODO - this seems wrong - should use
                        // `ProtocolConfig::is_last_block_before_activation`?
                        if era_id == block.header().next_block_era_id() {
                            match self.validator_matrix.validator_weights(era_id) {
                                Some(validator_weights) => {
                                    if self
                                        .storage
                                        .era_has_sufficient_finality_signatures(&validator_weights)
                                    {
                                        return CatchUpInstruction::CommitUpgrade(Box::new(
                                            block.header().clone(),
                                        ));
                                    }
                                }
                                None => {
                                    // should be unreachable
                                    return CatchUpInstruction::Shutdown(
                                        "should not be possible to be caught up with no era validators".to_string(),
                                    );
                                }
                            }
                        }
                    }
                    None => {
                        // should be unreachable
                        return CatchUpInstruction::Shutdown(
                            "can't be caught up with no block in the block store".to_string(),
                        );
                    }
                }
            }
        }

        // there are no catch up or shutdown instructions, so we must be caught up
        CatchUpInstruction::CaughtUp
    }

    fn should_we_validate(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Result<Option<Effects<MainEvent>>, String> {
        let highest_switch_block_header = match self.recent_switch_block_headers.last() {
            None => return Ok(None),
            Some(header) => header,
        };

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

        let highest_era_weights = match highest_switch_block_header.next_era_validator_weights() {
            None => {
                return Err(format!(
                    "highest switch block has no era end: {}",
                    highest_switch_block_header
                ))
            }
            Some(weights) => weights,
        };
        if !highest_era_weights.contains_key(self.consensus.public_key()) {
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
                self.contract_runtime.set_initial_state(initial_pre_state);

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

    pub(crate) fn commit_upgrade(
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
                    self.contract_runtime.set_initial_state(initial_pre_state);

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

    pub(crate) fn enqueue_executable_block(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Result<Effects<MainEvent>, String> {
        let mut effects = Effects::new();
        // TODO - try to avoid repeating executing the same blocks.
        if let Some(block_hash) = self.block_synchronizer.maybe_executable_block_hash() {
            // TODO: Why did we ask block accumulator here?
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
                Err(_) => {
                    return Err(format!(
                        "failure to make block and approvals hashes into executable block: {}",
                        block_hash
                    ));
                }
            }
        }
        Ok(effects)
    }
}
