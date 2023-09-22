use std::{
    fmt::{Display, Formatter},
    time::Duration,
};

use either::Either;
use tracing::{debug, error, info, warn};

use casper_execution_engine::engine_state::GetEraValidatorsError;
use casper_types::{ActivationPoint, BlockHash, BlockHeader, EraId, Timestamp};

use crate::{
    components::{
        block_accumulator::{SyncIdentifier, SyncInstruction},
        block_synchronizer::BlockSynchronizerProgress,
        contract_runtime::EraValidatorsRequest,
        storage::HighestOrphanedBlockResult,
        sync_leaper,
        sync_leaper::{LeapActivityError, LeapState},
    },
    effect::{
        requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, EffectResultExt, Effects,
    },
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{GlobalStatesMetadata, MaxTtl, SyncLeap, SyncLeapIdentifier},
    NodeRng,
};

pub(super) enum KeepUpInstruction {
    Validate(Effects<MainEvent>),
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    CatchUp,
    ShutdownForUpgrade,
    Fatal(String),
}

#[derive(Debug, Clone, Copy)]
enum SyncBackInstruction {
    Sync {
        sync_hash: BlockHash,
        sync_era: EraId,
    },
    Syncing,
    TtlSynced,
    GenesisSynced,
    NoSync,
}

impl Display for SyncBackInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncBackInstruction::Sync { sync_hash, .. } => {
                write!(f, "attempt to sync {}", sync_hash)
            }
            SyncBackInstruction::Syncing => write!(f, "syncing"),
            SyncBackInstruction::TtlSynced => write!(f, "ttl reached"),
            SyncBackInstruction::GenesisSynced => write!(f, "genesis reached"),
            SyncBackInstruction::NoSync => write!(f, "configured to not sync"),
        }
    }
}

impl MainReactor {
    pub(super) fn keep_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> KeepUpInstruction {
        if self.should_shutdown_for_upgrade() {
            // controlled shutdown for protocol upgrade.
            return KeepUpInstruction::ShutdownForUpgrade;
        }

        // if there is instruction, return to start working on it
        // else fall thru with the current best available id for block syncing
        let sync_identifier = match self.keep_up_process() {
            Either::Right(keep_up_instruction) => return keep_up_instruction,
            Either::Left(sync_identifier) => sync_identifier,
        };
        debug!(
            ?sync_identifier,
            "KeepUp: sync identifier {}",
            sync_identifier.block_hash()
        );
        // we check with the block accumulator before doing sync work as it may be aware of one or
        // more blocks that are higher than our current highest block
        let sync_instruction = self.block_accumulator.sync_instruction(sync_identifier);
        debug!(
            ?sync_instruction,
            "KeepUp: sync_instruction {}",
            sync_instruction.block_hash()
        );
        if let Some(keep_up_instruction) =
            self.keep_up_sync_instruction(effect_builder, sync_instruction)
        {
            return keep_up_instruction;
        }

        // we appear to be keeping up with the network and have some cycles to get other work done
        // check to see if we should attempt to sync a missing historical block (if any)
        debug!("KeepUp: keeping up with the network; try to sync an historical block");
        if let Some(keep_up_instruction) = self.sync_back_keep_up_instruction(effect_builder, rng) {
            return keep_up_instruction;
        }

        // we are keeping up, and don't need to sync an historical block; check to see if this
        // node should be participating in consensus this era (necessary for re-start scenarios)
        self.keep_up_should_validate(effect_builder, rng)
            .unwrap_or_else(|| {
                KeepUpInstruction::CheckLater(
                    "node is keeping up".to_string(),
                    self.control_logic_default_delay.into(),
                )
            })
    }

    fn keep_up_should_validate(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Option<KeepUpInstruction> {
        if let ActivationPoint::Genesis(genesis_timestamp) =
            self.chainspec.protocol_config.activation_point
        {
            // this is a non-validator node in KeepUp prior to genesis; there is no reason to
            // check consensus in this state, and it log spams if we do, so exiting early
            if genesis_timestamp > Timestamp::now() {
                return None;
            }
        }

        if self.sync_handling.is_no_sync() {
            // node is not permitted to be a validator with no_sync behavior.
            return None;
        }

        if self.block_synchronizer.forward_progress().is_active() {
            debug!("KeepUp: still syncing a block");
            return None;
        }

        let queue_depth = self.contract_runtime.queue_depth();
        if queue_depth > 0 {
            debug!("KeepUp: should_validate queue_depth {}", queue_depth);
            return None;
        }
        match self.create_required_eras(effect_builder, rng) {
            Ok(Some(effects)) => Some(KeepUpInstruction::Validate(effects)),
            Ok(None) => None,
            Err(msg) => Some(KeepUpInstruction::Fatal(msg)),
        }
    }

    fn keep_up_process(&mut self) -> Either<SyncIdentifier, KeepUpInstruction> {
        let forward_progress = self.block_synchronizer.forward_progress();
        self.update_last_progress(&forward_progress, false);
        match forward_progress {
            BlockSynchronizerProgress::Idle => {
                // not working on syncing a block (ready to start a new one)
                self.keep_up_idle()
            }
            BlockSynchronizerProgress::Syncing(block_hash, block_height, _) => {
                // working on syncing a block
                Either::Left(self.keep_up_syncing(block_hash, block_height))
            }
            // waiting for execution - forward only
            BlockSynchronizerProgress::Executing(block_hash, block_height, era_id) => {
                Either::Left(self.keep_up_executing(block_hash, block_height, era_id))
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                // for a synced forward block -> we have header, body, any referenced deploys,
                // sufficient finality (by weight) of signatures, associated global state and
                // execution effects.
                Either::Left(self.keep_up_synced(block_hash, block_height, era_id))
            }
        }
    }

    fn keep_up_idle(&mut self) -> Either<SyncIdentifier, KeepUpInstruction> {
        match self.storage.read_highest_complete_block() {
            Ok(Some(block)) => Either::Left(SyncIdentifier::LocalTip(
                *block.hash(),
                block.height(),
                block.era_id(),
            )),
            Ok(None) => {
                // something out of the ordinary occurred; it isn't legit to be in keep up mode
                // with no complete local blocks. go back to catch up which will either correct
                // or handle retry / shutdown behavior.
                error!("KeepUp: block synchronizer idle, local storage has no complete blocks");
                Either::Right(KeepUpInstruction::CatchUp)
            }
            Err(error) => Either::Right(KeepUpInstruction::Fatal(format!(
                "failed to read highest complete block: {}",
                error
            ))),
        }
    }

    fn keep_up_syncing(
        &mut self,
        block_hash: BlockHash,
        block_height: Option<u64>,
    ) -> SyncIdentifier {
        match block_height {
            None => SyncIdentifier::BlockHash(block_hash),
            Some(height) => SyncIdentifier::BlockIdentifier(block_hash, height),
        }
    }

    fn keep_up_executing(
        &mut self,
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
    ) -> SyncIdentifier {
        SyncIdentifier::ExecutingBlockIdentifier(block_hash, block_height, era_id)
    }

    fn keep_up_synced(
        &mut self,
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
    ) -> SyncIdentifier {
        debug!("KeepUp: synced block: {}", block_hash);
        // important: scrape forward synchronizer here to return it to idle status
        self.block_synchronizer.purge_forward();
        SyncIdentifier::SyncedBlockIdentifier(block_hash, block_height, era_id)
    }

    fn keep_up_sync_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        sync_instruction: SyncInstruction,
    ) -> Option<KeepUpInstruction> {
        match sync_instruction {
            SyncInstruction::Leap { .. } | SyncInstruction::LeapIntervalElapsed { .. } => {
                // the block accumulator is unsure what our block position is relative to the
                // network and wants to check peers for their notion of current tip.
                // to do this, we switch back to CatchUp which will engage the necessary
                // machinery to poll the network via the SyncLeap mechanic. if it turns out
                // we are actually at or near tip after all, we simply switch back to KeepUp
                // and continue onward. the accumulator is designed to periodically do this
                // if we've received no gossip about new blocks from peers within an interval.
                // this is to protect against partitioning and is not problematic behavior
                // when / if it occurs.
                Some(KeepUpInstruction::CatchUp)
            }
            SyncInstruction::BlockSync { block_hash } => {
                debug!("KeepUp: BlockSync: {:?}", block_hash);
                if self
                    .block_synchronizer
                    .register_block_by_hash(block_hash, false)
                {
                    info!(%block_hash, "KeepUp: BlockSync: registered block by hash");
                    Some(KeepUpInstruction::Do(
                        Duration::ZERO,
                        effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        }),
                    ))
                } else {
                    // this block has already been registered and is being worked on
                    None
                }
            }
            SyncInstruction::CaughtUp { .. } => {
                // the accumulator thinks we are at the tip of the network and we don't need
                // to do anything for the next one yet.
                None
            }
        }
    }

    fn sync_back_keep_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Option<KeepUpInstruction> {
        let sync_back_progress = self.block_synchronizer.historical_progress();
        debug!(?sync_back_progress, "KeepUp: historical sync back progress");
        self.update_last_progress(&sync_back_progress, true);
        match self.sync_back_instruction(&sync_back_progress) {
            Ok(Some(sbi @ sync_back_instruction)) => match sync_back_instruction {
                SyncBackInstruction::NoSync
                | SyncBackInstruction::GenesisSynced
                | SyncBackInstruction::TtlSynced => {
                    // we don't need to sync any historical blocks currently, so we clear both the
                    // historical synchronizer and the sync back leap activity since they will not
                    // be required anymore
                    debug!("KeepUp: {}", sbi);
                    self.block_synchronizer.purge_historical();
                    self.sync_leaper.purge();
                    None
                }
                SyncBackInstruction::Syncing => {
                    debug!("KeepUp: syncing historical; checking later");
                    Some(KeepUpInstruction::CheckLater(
                        format!("historical {}", SyncBackInstruction::Syncing),
                        self.control_logic_default_delay.into(),
                    ))
                }
                SyncBackInstruction::Sync {
                    sync_hash,
                    sync_era,
                } => {
                    debug!(%sync_hash, ?sync_era, validator_matrix_eras=?self.validator_matrix.eras(), "KeepUp: historical sync back instruction");
                    if self.validator_matrix.has_era(&sync_era) {
                        Some(self.sync_back_register(effect_builder, rng, sync_hash))
                    } else {
                        Some(self.sync_back_leap(effect_builder, rng, sync_hash))
                    }
                }
            },
            Ok(None) => None,
            Err(msg) => Some(KeepUpInstruction::Fatal(msg)),
        }
    }

    // Attempts to read the validators from the global states of the block after the upgrade and its
    // parent; initiates fetching of the missing global states, if any.
    fn try_read_validators_for_block_after_upgrade(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        global_states_metadata: GlobalStatesMetadata,
    ) -> KeepUpInstruction {
        // We try to read the validator sets from global states of two blocks - if either returns
        // `RootNotFound`, we'll initiate fetching of the corresponding global state.
        let effects = async move {
            // Send the requests to contract runtime.
            let before_era_validators_request = EraValidatorsRequest::new(
                global_states_metadata.before_state_hash,
                global_states_metadata.before_protocol_version,
            );
            let before_era_validators_result = effect_builder
                .get_era_validators_from_contract_runtime(before_era_validators_request)
                .await;
            let after_era_validators_request = EraValidatorsRequest::new(
                global_states_metadata.after_state_hash,
                global_states_metadata.after_protocol_version,
            );
            let after_era_validators_result = effect_builder
                .get_era_validators_from_contract_runtime(after_era_validators_request)
                .await;

            // Check the results.
            // A return value of `Ok` means that validators were read successfully.
            // An `Err` will contain a vector of (block_hash, global_state_hash) pairs to be
            // fetched by the `GlobalStateSynchronizer`, along with a vector of peers to ask.
            match (before_era_validators_result, after_era_validators_result) {
                // Both states were present - return the result.
                (Ok(before_era_validators), Ok(after_era_validators)) => {
                    Ok((before_era_validators, after_era_validators))
                }
                // Both were absent - fetch global states for both blocks.
                (
                    Err(GetEraValidatorsError::RootNotFound),
                    Err(GetEraValidatorsError::RootNotFound),
                ) => Err(vec![
                    (
                        global_states_metadata.before_hash,
                        global_states_metadata.before_state_hash,
                    ),
                    (
                        global_states_metadata.after_hash,
                        global_states_metadata.after_state_hash,
                    ),
                ]),
                // The after-block's global state was missing - return the hashes.
                (Ok(_), Err(GetEraValidatorsError::RootNotFound)) => Err(vec![(
                    global_states_metadata.after_hash,
                    global_states_metadata.after_state_hash,
                )]),
                // The before-block's global state was missing - return the hashes.
                (Err(GetEraValidatorsError::RootNotFound), Ok(_)) => Err(vec![(
                    global_states_metadata.before_hash,
                    global_states_metadata.before_state_hash,
                )]),
                // We got some error other than `RootNotFound` - just log the error and don't
                // synchronize anything.
                (before_result, after_result) => {
                    error!(
                        ?before_result,
                        ?after_result,
                        "couldn't read era validators from global state in block"
                    );
                    Err(vec![])
                }
            }
        }
        .result(
            // We got the era validators - just emit the event that will cause them to be compared,
            // validators matrix to be updated and reactor to be cranked.
            move |(before_era_validators, after_era_validators)| {
                MainEvent::GotBlockAfterUpgradeEraValidators(
                    global_states_metadata.after_era_id,
                    before_era_validators,
                    after_era_validators,
                )
            },
            // A global state was missing - we ask the BlockSynchronizer to fetch what is needed.
            |global_states_to_sync| {
                MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::SyncGlobalStates(
                    global_states_to_sync,
                ))
            },
        );
        // In either case, there are effects to be processed by the reactor.
        KeepUpInstruction::Do(Duration::ZERO, effects)
    }

    fn sync_back_leap(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
    ) -> KeepUpInstruction {
        // in this flow, we are leveraging the SyncLeap behavior to go backwards
        // rather than forwards. as we walk backwards from tip we know the block hash
        // of the parent of the earliest contiguous block we have locally (aka a
        // "parent_hash") but we don't know what era that parent block is in and we
        // may or may not know the validator set for that era to validate finality
        // signatures against. we use the leaper to gain awareness of the necessary
        // trusted ancestors to our earliest contiguous block to do necessary validation.
        let sync_back_status = self.sync_leaper.leap_status();
        info!(
            "KeepUp: historical sync back status {} {}",
            parent_hash, sync_back_status
        );
        debug!(
            ?parent_hash,
            ?sync_back_status,
            "KeepUp: historical sync back status"
        );
        match sync_back_status {
            LeapState::Idle => {
                debug!("KeepUp: historical sync back idle");
                self.sync_back_leaper_idle(effect_builder, rng, parent_hash, Duration::ZERO)
            }
            LeapState::Awaiting { .. } => KeepUpInstruction::CheckLater(
                "KeepUp: historical sync back is awaiting response".to_string(),
                self.control_logic_default_delay.into(),
            ),
            LeapState::Received {
                best_available,
                from_peers: _,
                ..
            } => self.sync_back_leap_received(effect_builder, *best_available),
            LeapState::Failed { error, .. } => {
                self.sync_back_leap_failed(effect_builder, rng, parent_hash, error)
            }
        }
    }

    fn sync_back_leap_failed(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
        error: LeapActivityError,
    ) -> KeepUpInstruction {
        warn!(
            %error,
            "KeepUp: failed historical sync back",
        );
        self.sync_back_leaper_idle(
            effect_builder,
            rng,
            parent_hash,
            self.control_logic_default_delay.into(),
        )
    }

    fn sync_back_leaper_idle(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
        offset: Duration,
    ) -> KeepUpInstruction {
        // we get a random sampling of peers to ask.
        let peers_to_ask = self.net.fully_connected_peers_random(
            rng,
            self.chainspec.core_config.simultaneous_peer_requests as usize,
        );
        if peers_to_ask.is_empty() {
            return KeepUpInstruction::CheckLater(
                "no peers".to_string(),
                self.control_logic_default_delay.into(),
            );
        }

        // latch accumulator progress to allow sync-leap time to do work
        self.block_accumulator.reset_last_progress();

        let sync_leap_identifier = SyncLeapIdentifier::sync_to_historical(parent_hash);

        let effects = effect_builder.immediately().event(move |_| {
            MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                sync_leap_identifier,
                peers_to_ask,
            })
        });
        KeepUpInstruction::Do(offset, effects)
    }

    fn sync_back_leap_received(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        sync_leap: SyncLeap,
    ) -> KeepUpInstruction {
        // use the leap response to update our recent switch block data (if relevant) and
        // era validator weights. if there are other processes which are holding on discovery
        // of relevant newly-seen era validator weights, they should naturally progress
        // themselves via notification on the event loop.
        let block_hash = sync_leap.highest_block_hash();
        let block_height = sync_leap.highest_block_height();
        info!(%sync_leap, %block_height, %block_hash, "KeepUp: historical sync_back received");

        let era_validator_weights = sync_leap.era_validator_weights(
            self.validator_matrix.fault_tolerance_threshold(),
            &self.chainspec.protocol_config,
        );
        for evw in era_validator_weights {
            let era_id = evw.era_id();
            debug!(%era_id, "KeepUp: attempt to register historical validators for era");
            if self.validator_matrix.register_era_validator_weights(evw) {
                info!("KeepUp: got historical era {}", era_id);
            } else {
                debug!(%era_id, "KeepUp: historical era already present or is not relevant");
            }
        }

        if let Some(global_states_metadata) = sync_leap.global_states_for_sync_across_upgrade() {
            self.try_read_validators_for_block_after_upgrade(effect_builder, global_states_metadata)
        } else {
            KeepUpInstruction::CheckLater(
                "historical sync back received".to_string(),
                Duration::ZERO,
            )
        }
    }

    fn sync_back_register(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
    ) -> KeepUpInstruction {
        if self
            .block_synchronizer
            .register_block_by_hash(parent_hash, true)
        {
            // sync the parent_hash block; we get a random sampling of peers to ask.
            // it is possible that we may get a random sampling that do not have the data
            // we need, but the synchronizer should (eventually) detect that and ask for
            // more peers via the NeedNext behavior.
            let peers_to_ask = self.net.fully_connected_peers_random(
                rng,
                self.chainspec.core_config.simultaneous_peer_requests as usize,
            );
            debug!(
                "KeepUp: historical register_block_by_hash: {} peers count: {:?}",
                parent_hash,
                peers_to_ask.len()
            );
            self.block_synchronizer
                .register_peers(parent_hash, peers_to_ask);
            KeepUpInstruction::Do(
                Duration::ZERO,
                effect_builder.immediately().event(|_| {
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                }),
            )
        } else {
            KeepUpInstruction::CheckLater(
                format!("historical syncing {}", parent_hash),
                self.control_logic_default_delay.into(),
            )
        }
    }

    fn sync_back_instruction(
        &mut self,
        block_synchronizer_progress: &BlockSynchronizerProgress,
    ) -> Result<Option<SyncBackInstruction>, String> {
        match block_synchronizer_progress {
            BlockSynchronizerProgress::Syncing(_, _, _) => {
                debug!("KeepUp: still syncing historical block");
                return Ok(Some(SyncBackInstruction::Syncing));
            }
            BlockSynchronizerProgress::Executing(block_hash, height, _) => {
                warn!(
                    %block_hash,
                    %height,
                    "Historical block synchronizer should not be waiting for the block to be executed"
                );
            }
            BlockSynchronizerProgress::Idle | BlockSynchronizerProgress::Synced(_, _, _) => {}
        }
        // in this flow there is no significant difference between Idle & Synced, as unlike in
        // catchup and keepup flows there is no special activity necessary upon getting to Synced
        // on an old block. in either case we will attempt to get the next needed block (if any).
        // note: for a synced historical block we have header, body, global state, any execution
        // effects, any referenced deploys, & sufficient finality (by weight) of signatures.
        match self.storage.get_highest_orphaned_block_header() {
            HighestOrphanedBlockResult::Orphan(highest_orphaned_block_header) => {
                if let Some(synched) = self.synched(&highest_orphaned_block_header)? {
                    return Ok(Some(synched));
                }
                let (sync_hash, sync_era) =
                    self.sync_hash_and_era(&highest_orphaned_block_header)?;
                debug!(?sync_era, %sync_hash, "KeepUp: historical sync target era and block hash");

                self.validator_matrix
                    .register_retrograde_latch(Some(sync_era));
                Ok(Some(SyncBackInstruction::Sync {
                    sync_hash,
                    sync_era,
                }))
            }
            HighestOrphanedBlockResult::MissingFromBlockHeightIndex(block_height) => Err(format!(
                "KeepUp: storage is missing historical block height index entry {}",
                block_height
            )),
            HighestOrphanedBlockResult::MissingHeader(block_hash) => Err(format!(
                "KeepUp: storage is missing historical block header for {}",
                block_hash
            )),
            HighestOrphanedBlockResult::MissingHighestSequence => {
                Err("KeepUp: storage is missing historical highest block sequence".to_string())
            }
        }
    }

    fn synched(
        &self,
        highest_orphaned_block_header: &BlockHeader,
    ) -> Result<Option<SyncBackInstruction>, String> {
        // if we're configured to not sync, don't sync.
        if self.sync_handling.is_no_sync() {
            return Ok(Some(SyncBackInstruction::NoSync));
        }

        // if we've reached genesis, there's nothing left to sync.
        if highest_orphaned_block_header.is_genesis() {
            return Ok(Some(SyncBackInstruction::GenesisSynced));
        }

        if self.sync_handling.is_sync_to_genesis() {
            return Ok(None);
        }

        // if sync to genesis is false, we require sync to ttl; i.e. if the TTL is 18
        // hours we require sync back to see a contiguous / unbroken
        // range of at least 18 hours worth of blocks. note however
        // that we measure from the start of the active era (for consensus reasons),
        // so this can be up to TTL + era length in practice

        if let Some(highest_switch_block_header) = self
            .storage
            .read_highest_switch_block_headers(1)
            .map_err(|err| err.to_string())?
            .last()
        {
            let max_ttl: MaxTtl = self.chainspec.transaction_config.max_ttl.into();
            if max_ttl.synced_to_ttl(
                highest_switch_block_header.timestamp(),
                highest_orphaned_block_header,
            ) {
                return Ok(Some(SyncBackInstruction::TtlSynced));
            }
        }

        Ok(None)
    }

    fn sync_hash_and_era(
        &self,
        highest_orphaned_block_header: &BlockHeader,
    ) -> Result<(BlockHash, EraId), String> {
        let parent_hash = highest_orphaned_block_header.parent_hash();
        debug!(?highest_orphaned_block_header, %parent_hash, "KeepUp: highest orphaned historical block");

        // if we are in genesis era but do not have validators loaded for genesis era,
        // attempt to skip to switch block of era 1 and leap from there; other validators
        // must cite era 0 to prove trusted ancestors for era 1, which will resolve the issue
        // when received by this node.
        if highest_orphaned_block_header.era_id().is_genesis()
            && !self
                .validator_matrix
                .has_era(&highest_orphaned_block_header.era_id())
        {
            match self
                .storage
                .read_switch_block_by_era_id(highest_orphaned_block_header.era_id().successor())
            {
                Ok(Some(switch)) => {
                    debug!(
                        ?highest_orphaned_block_header,
                        "KeepUp: historical sync in genesis era attempting correction for unmatrixed genesis validators"
                    );
                    return Ok((switch.header().block_hash(), switch.header().era_id()));
                }
                Ok(None) => return Err(
                    "In genesis era with no genesis validators and missing next era switch block"
                        .to_string(),
                ),
                Err(err) => return Err(err.to_string()),
            }
        }

        match self.storage.read_block_header(parent_hash) {
            Ok(Some(parent_block_header)) => {
                // even if we don't have a complete block (all parts and dependencies)
                // we may have the parent's block header; if we do we also
                // know its era which allows us to know if we have the validator
                // set for that era or not
                debug!(
                    ?parent_block_header,
                    "KeepUp: historical sync found parent block header in storage"
                );
                Ok((
                    parent_block_header.block_hash(),
                    parent_block_header.era_id(),
                ))
            }
            Ok(None) => {
                debug!(%parent_hash, "KeepUp: historical sync did not find block header in storage");
                let era_id = match highest_orphaned_block_header.era_id().predecessor() {
                    None => EraId::from(0),
                    Some(predecessor) => {
                        // we do not have the parent header and thus don't know what era
                        // the parent block is in (it could be the same era or the previous
                        // era). we assume the worst case and ask for the earlier era's
                        // proof; subtracting 1 here is safe
                        // since the case where era id is 0 is
                        // handled above
                        predecessor
                    }
                };
                Ok((*parent_hash, era_id))
            }
            Err(err) => Err(err.to_string()),
        }
    }
}

#[cfg(test)]
pub(crate) fn synced_to_ttl(
    latest_switch_block_header: &BlockHeader,
    highest_orphaned_block_header: &BlockHeader,
    max_ttl: casper_types::TimeDiff,
) -> Result<bool, String> {
    Ok(highest_orphaned_block_header.height() == 0
        || is_timestamp_at_ttl(
            latest_switch_block_header.timestamp(),
            highest_orphaned_block_header.timestamp(),
            max_ttl,
        ))
}

#[cfg(test)]
fn is_timestamp_at_ttl(
    latest_switch_block_timestamp: Timestamp,
    lowest_block_timestamp: Timestamp,
    max_ttl: casper_types::TimeDiff,
) -> bool {
    lowest_block_timestamp < latest_switch_block_timestamp.saturating_sub(max_ttl)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use casper_types::{testing::TestRng, TestBlockBuilder, TimeDiff, Timestamp};

    use crate::reactor::main_reactor::keep_up::{is_timestamp_at_ttl, synced_to_ttl};

    const TWO_DAYS_SECS: u32 = 60 * 60 * 24 * 2;
    const MAX_TTL: TimeDiff = TimeDiff::from_seconds(86400);

    #[test]
    fn should_be_at_ttl() {
        let latest_switch_block_timestamp = Timestamp::from_str("2010-06-15 00:00:00.000").unwrap();
        let lowest_block_timestamp = Timestamp::from_str("2010-06-10 00:00:00.000").unwrap();
        let max_ttl = TimeDiff::from_seconds(TWO_DAYS_SECS);
        assert!(is_timestamp_at_ttl(
            latest_switch_block_timestamp,
            lowest_block_timestamp,
            max_ttl
        ));
    }

    #[test]
    fn should_not_be_at_ttl() {
        let latest_switch_block_timestamp = Timestamp::from_str("2010-06-15 00:00:00.000").unwrap();
        let lowest_block_timestamp = Timestamp::from_str("2010-06-14 00:00:00.000").unwrap();
        let max_ttl = TimeDiff::from_seconds(TWO_DAYS_SECS);
        assert!(!is_timestamp_at_ttl(
            latest_switch_block_timestamp,
            lowest_block_timestamp,
            max_ttl
        ));
    }

    #[test]
    fn should_detect_ttl_at_the_boundary() {
        let latest_switch_block_timestamp = Timestamp::from_str("2010-06-15 00:00:00.000").unwrap();
        let lowest_block_timestamp = Timestamp::from_str("2010-06-12 23:59:59.999").unwrap();
        let max_ttl = TimeDiff::from_seconds(TWO_DAYS_SECS);
        assert!(is_timestamp_at_ttl(
            latest_switch_block_timestamp,
            lowest_block_timestamp,
            max_ttl
        ));

        let latest_switch_block_timestamp = Timestamp::from_str("2010-06-15 00:00:00.000").unwrap();
        let lowest_block_timestamp = Timestamp::from_str("2010-06-13 00:00:00.000").unwrap();
        let max_ttl = TimeDiff::from_seconds(TWO_DAYS_SECS);
        assert!(!is_timestamp_at_ttl(
            latest_switch_block_timestamp,
            lowest_block_timestamp,
            max_ttl
        ));

        let latest_switch_block_timestamp = Timestamp::from_str("2010-06-15 00:00:00.000").unwrap();
        let lowest_block_timestamp = Timestamp::from_str("2010-06-13 00:00:00.001").unwrap();
        let max_ttl = TimeDiff::from_seconds(TWO_DAYS_SECS);
        assert!(!is_timestamp_at_ttl(
            latest_switch_block_timestamp,
            lowest_block_timestamp,
            max_ttl
        ));
    }

    #[test]
    fn should_detect_ttl_at_genesis() {
        let rng = &mut TestRng::new();

        let latest_switch_block = TestBlockBuilder::new()
            .era(100)
            .height(1000)
            .switch_block(true)
            .build(rng);

        let latest_orphaned_block = TestBlockBuilder::new()
            .era(0)
            .height(0)
            .switch_block(true)
            .build(rng);

        assert_eq!(latest_orphaned_block.height(), 0);
        assert_eq!(
            synced_to_ttl(
                latest_switch_block.header(),
                latest_orphaned_block.header(),
                MAX_TTL
            ),
            Ok(true)
        );
    }
}
