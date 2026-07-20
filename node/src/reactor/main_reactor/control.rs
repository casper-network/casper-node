use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::runtime::Handle;
use tracing::{debug, error, info, trace};

use casper_storage::data_access_layer::GenesisResult;
use casper_types::{
    BlockHash, BlockHeader, Chainspec, ChainspecRawBytes, Digest, EraId, PublicKey, TimeDiff,
    Timestamp,
};

use crate::{
    components::{
        binary_port,
        block_synchronizer::{self, BlockSynchronizerProgress},
        contract_runtime::{ContractRuntime, ExecutionPreState},
        diagnostics_port, event_stream_server, network, rest_server, storage, upgrade_watcher,
    },
    effect::{announcements::ControlAnnouncement, EffectBuilder, EffectExt, Effects},
    fatal,
    reactor::main_reactor::{
        catch_up::CatchUpInstruction, genesis_instruction::GenesisInstruction,
        keep_up::KeepUpInstruction, upgrade_shutdown::UpgradeShutdownInstruction, utils,
        validate::ValidateInstruction, Error, MainEvent, MainReactor, PendingImmediateSwitchBlock,
        ReactorState,
    },
    types::{BlockPayload, ExecutableBlock, FinalizedBlock, InternalEraReport, MetaBlockState},
    NodeRng,
};

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
                    tokio::time::sleep(delay).await;
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
        const INITIALIZATION_DELAY_SPEED_UP_FACTOR: u64 = 4;

        match self.state {
            ReactorState::Initialize => {
                // We can be more greedy when cranking through the initialization process as the
                // progress is expected to happen quickly.
                let initialization_logic_default_delay =
                    self.control_logic_default_delay / INITIALIZATION_DELAY_SPEED_UP_FACTOR;

                match self.initialize_next_component(effect_builder) {
                    Some(effects) => (initialization_logic_default_delay.into(), effects),
                    None => {
                        if self.sync_handling.is_isolated() {
                            // If node is "isolated" it doesn't care about peers
                            let effects = match self
                                .refresh_contract_runtime_or_finish_upgrade(effect_builder)
                            {
                                Ok(effects) => effects,
                                Err(msg) => {
                                    return (
                                        Duration::ZERO,
                                        fatal!(effect_builder, "{}", msg).ignore(),
                                    )
                                }
                            };
                            self.state = ReactorState::KeepUp;
                            return (Duration::ZERO, effects);
                        }
                        if false == self.net.has_sufficient_fully_connected_peers() {
                            info!("Initialize: awaiting sufficient fully-connected peers");
                            return (initialization_logic_default_delay.into(), Effects::new());
                        }
                        let effects = match self
                            .refresh_contract_runtime_or_finish_upgrade(effect_builder)
                        {
                            Ok(effects) => effects,
                            Err(msg) => {
                                return (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                            }
                        };
                        info!("Initialize: switch to CatchUp");
                        self.state = ReactorState::CatchUp;
                        (Duration::ZERO, effects)
                    }
                }
            }
            ReactorState::CatchUp => match self.catch_up_instruction(effect_builder, rng) {
                CatchUpInstruction::Fatal(msg) => {
                    (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                }
                CatchUpInstruction::ShutdownForUpgrade => {
                    info!("CatchUp: shutting down for upgrade");
                    self.switch_to_shutdown_for_upgrade();
                    (Duration::ZERO, Effects::new())
                }
                CatchUpInstruction::CommitGenesis => match self.commit_genesis(effect_builder) {
                    GenesisInstruction::Validator(duration, effects) => {
                        info!("CatchUp: switch to Validate at genesis");
                        self.block_synchronizer.purge();
                        self.state = ReactorState::Validate;
                        (duration, effects)
                    }
                    GenesisInstruction::NonValidator(duration, effects) => {
                        info!("CatchUp: non-validator committed genesis");
                        self.state = ReactorState::CatchUp;
                        (duration, effects)
                    }
                    GenesisInstruction::Fatal(msg) => (
                        Duration::ZERO,
                        fatal!(effect_builder, "failed to commit genesis: {}", msg).ignore(),
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
                    // shut down instead of switching to KeepUp if catch up and shutdown mode is
                    // enabled
                    if self.sync_handling.is_complete_block() {
                        info!("CatchUp: immediate shutdown after catching up");
                        self.state = ReactorState::ShutdownAfterCatchingUp;
                        (Duration::ZERO, Effects::new())
                    } else {
                        // purge to avoid polluting the status endpoints w/ stale state
                        info!("CatchUp: switch to KeepUp");
                        self.block_synchronizer.purge();
                        self.state = ReactorState::KeepUp;
                        (Duration::ZERO, Effects::new())
                    }
                }
            },
            ReactorState::KeepUp => match self.keep_up_instruction(effect_builder, rng) {
                KeepUpInstruction::Fatal(msg) => {
                    (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore())
                }
                KeepUpInstruction::ShutdownForUpgrade => {
                    info!("KeepUp: switch to ShutdownForUpgrade");
                    self.switch_to_shutdown_for_upgrade();
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
                    self.sync_leaper.purge();
                    info!("KeepUp: switch to CatchUp");
                    self.state = ReactorState::CatchUp;
                    (Duration::ZERO, Effects::new())
                }
                KeepUpInstruction::Validate(effects) => {
                    if let Err(msg) = self.refresh_contract_runtime() {
                        return (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore());
                    }
                    info!("KeepUp: switch to Validate");
                    // purge to avoid polluting the status endpoints w/ stale state
                    self.block_synchronizer.purge();
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
                    self.switch_to_shutdown_for_upgrade();
                    (Duration::ZERO, Effects::new())
                }
                ValidateInstruction::CheckLater(msg, wait) => {
                    debug!("Validate: {}", msg);
                    (wait, Effects::new())
                }
                ValidateInstruction::Do(wait, effects) => {
                    trace!("Validate: node is processing effects");
                    (wait, effects)
                }
                ValidateInstruction::CatchUp => match self.deactivate_consensus_voting() {
                    Ok(_) => {
                        info!("Validate: switch to CatchUp");
                        self.state = ReactorState::CatchUp;
                        (Duration::ZERO, Effects::new())
                    }
                    Err(msg) => (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore()),
                },
                ValidateInstruction::KeepUp => match self.deactivate_consensus_voting() {
                    Ok(_) => {
                        info!("Validate: switch to KeepUp");
                        self.state = ReactorState::KeepUp;
                        (Duration::ZERO, Effects::new())
                    }
                    Err(msg) => (Duration::ZERO, fatal!(effect_builder, "{}", msg).ignore()),
                },
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
            ReactorState::ShutdownAfterCatchingUp => {
                let effects = effect_builder.immediately().event(|()| {
                    MainEvent::ControlAnnouncement(ControlAnnouncement::ShutdownAfterCatchingUp)
                });
                (Duration::ZERO, effects)
            }
        }
    }

    // NOTE: the order in which components are initialized is purposeful,
    // so don't alter the order without understanding the semantics
    fn initialize_next_component(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Option<Effects<MainEvent>> {
        // storage must be ready before anything else touches disk-backed state (other
        // components, e.g. transaction_buffer, read from it during their own init).
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.storage,
            MainEvent::Storage(storage::Event::Initialize),
        ) {
            return Some(effects);
        }
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

        // initialize transaction buffer from local storage; on a new node this is nearly a noop
        // but on a restarting node it can be relatively time consuming (depending upon TTL and
        // how many transactions there have been within the TTL)
        if let Some(effects) = self
            .transaction_buffer
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

        // bring up rpc and rest server last to defer complications (such as put_transaction) and
        // for it to be able to answer to /status, which requires various other components to be
        // initialized
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.rest_server,
            MainEvent::RestServer(rest_server::Event::Initialize),
        ) {
            return Some(effects);
        }

        // bring up binary port
        if let Some(effects) = utils::initialize_component(
            effect_builder,
            &mut self.binary_port,
            MainEvent::BinaryPort(binary_port::Event::Initialize),
        ) {
            return Some(effects);
        }

        None
    }

    fn commit_genesis(&mut self, effect_builder: EffectBuilder<MainEvent>) -> GenesisInstruction {
        let genesis_timestamp = match self
            .chainspec
            .protocol_config
            .activation_point
            .genesis_timestamp()
        {
            None => {
                return GenesisInstruction::Fatal(
                    "CommitGenesis: invalid chainspec activation point".to_string(),
                );
            }
            Some(timestamp) => timestamp,
        };

        // global state starts empty and gets populated based upon chainspec artifacts
        let post_state_hash = match self.contract_runtime.commit_genesis(
            self.chainspec.clone().as_ref(),
            self.chainspec_raw_bytes.clone().as_ref(),
        ) {
            GenesisResult::Fatal(msg) => {
                return GenesisInstruction::Fatal(msg);
            }
            GenesisResult::Failure(err) => {
                return GenesisInstruction::Fatal(format!("genesis error: {}", err));
            }
            GenesisResult::Success {
                post_state_hash, ..
            } => post_state_hash,
        };

        info!(
            %post_state_hash,
            %genesis_timestamp,
            network_name = %self.chainspec.network_config.name,
            "CommitGenesis: successful commit; initializing contract runtime"
        );

        let genesis_block_height = 0;
        self.initialize_contract_runtime(
            genesis_block_height,
            post_state_hash,
            BlockHash::default(),
            Digest::default(),
        );

        let era_id = EraId::default();

        // as this is a genesis validator, there is no historical syncing necessary
        // thus, the retrograde latch is immediately set
        self.validator_matrix
            .register_retrograde_latch(Some(era_id));

        // new networks will create a switch block at genesis to
        // surface the genesis validators. older networks did not
        // have this behavior.
        let genesis_switch_block = FinalizedBlock::new(
            BlockPayload::default(),
            Some(InternalEraReport::default()),
            genesis_timestamp,
            era_id,
            genesis_block_height,
            PublicKey::System,
        );

        // this genesis block has no transactions, and will get
        // handed off to be stored & marked complete after
        // sufficient finality signatures have been collected.
        let effects = effect_builder
            .enqueue_block_for_execution(
                ExecutableBlock::from_finalized_block_and_transactions(
                    genesis_switch_block,
                    vec![],
                ),
                MetaBlockState::new_not_to_be_gossiped(),
            )
            .ignore();

        if self
            .chainspec
            .network_config
            .accounts_config
            .is_genesis_validator(self.validator_matrix.public_signing_key())
        {
            // validators should switch over and start making blocks
            GenesisInstruction::Validator(Duration::ZERO, effects)
        } else {
            // non-validators should start receiving gossip about the block at height 1 soon
            GenesisInstruction::NonValidator(self.control_logic_default_delay.into(), effects)
        }
    }

    /// If `tip_header` is a switch block that is the last block before the chainspec's
    /// activation point, synchronously commits the protocol upgrade against `contract_runtime`'s
    /// global state. Returns the info needed to later produce, sign, and gossip the resulting
    /// immediate switch block, once the reactor is ready to do so (see
    /// [`Self::maybe_finish_pending_upgrade`]). Returns `Ok(None)` if no upgrade is due.
    ///
    /// This is an associated function (rather than a `&self` method) so it can be called from
    /// `MainReactor::new`, before the reactor itself has been constructed -- that's the only
    /// call site: a fresh restart whose local tip already sits at the pre-activation switch
    /// block (e.g. after a live node shuts itself down for the upgrade). A node still *catching
    /// up* through a historical activation point does not go through here; it just receives the
    /// post-upgrade chain via the ordinary block-synchronizer fetch path, like any other
    /// historical data.
    pub(super) fn commit_upgrade_if_needed(
        contract_runtime: &ContractRuntime,
        chainspec: &Arc<Chainspec>,
        chainspec_raw_bytes: &Arc<ChainspecRawBytes>,
        tip_header: Option<&BlockHeader>,
        upgrade_timeout: TimeDiff,
    ) -> Result<Option<PendingImmediateSwitchBlock>, Error> {
        let Some(tip_header) = tip_header else {
            return Ok(None);
        };
        if !(tip_header.is_switch_block()
            && tip_header.is_last_block_before_activation(&chainspec.protocol_config))
        {
            return Ok(None);
        }

        info!(
            era_id = %tip_header.era_id(),
            height = tip_header.height(),
            "committing protocol upgrade"
        );

        let upgrade_config = chainspec
            .upgrade_config_from_parts(
                *tip_header.state_root_hash(),
                tip_header.protocol_version(),
                chainspec.protocol_config.activation_point.era_id(),
                chainspec_raw_bytes.clone(),
            )
            .map_err(Error::ProtocolUpgrade)?;

        // Executing protocol upgrade can be time consuming. It's executed in the background so the
        // upgrade_timeout can be enforced. This function stays synchronous -- it's called
        // from `MainReactor::new`, before the reactor's async event loop exists -- so the
        // wait for that bounded future to resolve is bridged onto a dedicated scoped
        // thread, which calls `Handle::block_on` directly.
        let handle = Handle::current();
        let post_state_hash = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    handle.block_on(async {
                        match tokio::time::timeout(
                            Duration::from(upgrade_timeout),
                            contract_runtime.commit_protocol_upgrade(upgrade_config),
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(format!(
                                "protocol upgrade did not complete within {}",
                                upgrade_timeout
                            )),
                        }
                    })
                })
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
        .map_err(Error::ProtocolUpgrade)?;

        Ok(Some(PendingImmediateSwitchBlock {
            next_block_height: tip_header.height() + 1,
            post_state_hash,
            parent_hash: tip_header.block_hash(),
            parent_seed: *tip_header.accumulated_seed(),
            era_id: tip_header.next_block_era_id(),
            // Adding one second here to make sure the timestamp is monotonically growing -
            // it's important for EVM smart contracts
            timestamp: tip_header
                .timestamp()
                .saturating_add(TimeDiff::from_seconds(1)),
        }))
    }

    /// If a protocol upgrade has been committed and its immediate switch block hasn't yet been
    /// produced, builds the effects to enqueue it for execution, which will get it signed (by
    /// this validator, if applicable) and gossiped through the normal block-execution pipeline
    /// (see `main_reactor::handle_meta_block`).
    ///
    /// The caller MUST NOT invoke this before the node can actually reach peers -- broadcasting a
    /// finality signature to zero connected peers silently drops it with no retry (see
    /// `network::broadcast_message_to_validators`). This is why callers only invoke it once the
    /// existing `Initialize` peer-gate (`has_sufficient_fully_connected_peers`, or isolated mode)
    /// has passed, or from within `CatchUp`, which is only reachable after that same gate.
    pub(super) fn maybe_finish_pending_upgrade(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Option<Effects<MainEvent>> {
        let pending = self.pending_immediate_switch_block.take()?;
        self.upgrade_started_at = Some(Timestamp::now());
        self.contract_runtime
            .set_execution_pre_state(ExecutionPreState::new(
                pending.next_block_height,
                pending.post_state_hash,
                pending.parent_hash,
                pending.parent_seed,
            ));

        let current_price = self.contract_runtime.current_gas_price();
        let payload = BlockPayload::new(
            BTreeMap::new(),
            vec![],
            Default::default(),
            false,
            current_price,
        );
        let finalized_block = FinalizedBlock::new(
            payload,
            Some(InternalEraReport::default()),
            pending.timestamp,
            pending.era_id,
            pending.next_block_height,
            PublicKey::System,
        );

        info!("producing immediate switch block after protocol upgrade");

        Some(
            effect_builder
                .enqueue_block_for_execution(
                    ExecutableBlock::from_finalized_block_and_transactions(finalized_block, vec![]),
                    MetaBlockState::new_not_to_be_gossiped(),
                )
                .ignore(),
        )
    }

    /// Either finishes a pending protocol upgrade (producing its immediate switch block) or, if
    /// none is pending, refreshes contract runtime's execution pre-state from the local tip as
    /// usual. Used at the two points the reactor exits `ReactorState::Initialize`.
    fn refresh_contract_runtime_or_finish_upgrade(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> Result<Effects<MainEvent>, String> {
        if let Some(effects) = self.maybe_finish_pending_upgrade(effect_builder) {
            return Ok(effects);
        }
        self.refresh_contract_runtime()?;
        Ok(Effects::new())
    }

    pub(super) fn should_shutdown_for_upgrade(&self) -> bool {
        let recent_switch_block_headers = match self.storage.read_highest_switch_block_headers(1) {
            Ok(headers) => headers,
            Err(error) => {
                error!(
                    "{:?}: error getting recent switch block headers: {}",
                    self.state, error
                );
                return false;
            }
        };

        if let Some(block_header) = recent_switch_block_headers.last() {
            let highest_block_complete =
                self.storage.highest_complete_block_height() == Some(block_header.height());
            return highest_block_complete
                && self
                    .upgrade_watcher
                    .should_upgrade_after(block_header.era_id());
        }
        false
    }

    pub(super) fn should_commit_upgrade(&self) -> bool {
        match self.get_local_tip_header() {
            Ok(Some(block_header)) if block_header.is_switch_block() => {
                block_header.is_last_block_before_activation(&self.chainspec.protocol_config)
            }
            Ok(Some(_) | None) => false,
            Err(msg) => {
                error!("{:?}: {}", self.state, msg);
                false
            }
        }
    }

    fn refresh_contract_runtime(&mut self) -> Result<(), String> {
        if let Some(block_header) = self.get_local_tip_header()? {
            let block_height = block_header.height();
            let state_root_hash = block_header.state_root_hash();
            let block_hash = block_header.block_hash();
            let accumulated_seed = *block_header.accumulated_seed();
            self.initialize_contract_runtime(
                block_height + 1,
                *state_root_hash,
                block_hash,
                accumulated_seed,
            );
        }
        Ok(())
    }

    fn initialize_contract_runtime(
        &mut self,
        next_block_height: u64,
        pre_state_root_hash: Digest,
        parent_hash: BlockHash,
        parent_seed: Digest,
    ) {
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
            .set_execution_pre_state(initial_pre_state);
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

    fn deactivate_consensus_voting(&mut self) -> Result<(), String> {
        let deactivated_era_id = self.consensus.deactivate_current_era()?;
        info!(
            era_id = %deactivated_era_id,
            "{:?}: consensus deactivated",
            self.state
        );
        Ok(())
    }

    fn switch_to_shutdown_for_upgrade(&mut self) {
        self.state = ReactorState::ShutdownForUpgrade;
        self.switched_to_shutdown_for_upgrade = Timestamp::now();
    }

    fn get_local_tip_header(&self) -> Result<Option<BlockHeader>, String> {
        match self
            .storage
            .get_highest_complete_block()
            .map_err(|err| format!("Could not read highest complete block: {}", err))?
        {
            Some(local_tip) => Ok(Some(local_tip.take_header())),
            None => Ok(None),
        }
    }
}
