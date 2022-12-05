use either::Either;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use casper_types::EraId;

use crate::{
    components::{
        block_accumulator::{StartingWith, SyncInstruction},
        block_synchronizer::BlockSynchronizerProgress,
        sync_leaper,
        sync_leaper::{LeapActivityError, LeapStatus},
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{BlockHash, Item, SyncLeap, SyncLeapIdentifier},
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

impl MainReactor {
    pub(super) fn keep_up_instruction(
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

        let starting_with = match self.keep_up_process() {
            Either::Right(keep_up_instruction) => return keep_up_instruction,
            Either::Left(starting_with) => starting_with,
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
        if let Some(keep_up_instruction) =
            self.keep_up_sync_instruction(effect_builder, sync_instruction)
        {
            return keep_up_instruction;
        }
        match self.sync_to_historical {
            true => self.sync_back_process(effect_builder, rng),
            false => KeepUpInstruction::CheckLater(
                "at perceived tip of chain".to_string(),
                self.control_logic_default_delay.into(),
            ),
        }
    }

    fn keep_up_process(&mut self) -> Either<StartingWith, KeepUpInstruction> {
        let forward_progress = self.block_synchronizer.forward_progress();
        self.update_last_progress(&forward_progress, false);
        match forward_progress {
            BlockSynchronizerProgress::Idle => self.keep_up_idle(),
            BlockSynchronizerProgress::Syncing(block_hash, block_height, _) => {
                Either::Left(self.keep_up_syncing(block_hash, block_height))
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                Either::Left(self.keep_up_synced(block_hash, block_height, era_id))
            }
        }
    }

    fn keep_up_idle(&mut self) -> Either<StartingWith, KeepUpInstruction> {
        match self.storage.read_highest_complete_block() {
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
                    Ok(_) => Either::Left(StartingWith::LocalTip(
                        block.id(),
                        block_height,
                        block.header().era_id(),
                    )),
                    Err(msg) => Either::Right(KeepUpInstruction::Fatal(msg)),
                }
            }
            Ok(None) => {
                error!("KeepUp: block synchronizer idle, local storage has no complete blocks");
                self.block_synchronizer.purge();
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
    ) -> StartingWith {
        match block_height {
            None => StartingWith::Hash(block_hash),
            Some(height) => StartingWith::BlockIdentifier(block_hash, height),
        }
    }

    fn keep_up_synced(
        &mut self,
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
    ) -> StartingWith {
        debug!("KeepUp: synced block: {}", block_hash);
        self.block_synchronizer.purge_forward();
        StartingWith::SyncedBlockIdentifier(block_hash, block_height, era_id)
    }

    fn keep_up_sync_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        sync_instruction: SyncInstruction,
    ) -> Option<KeepUpInstruction> {
        match sync_instruction {
            SyncInstruction::Leap { .. } => {
                self.block_synchronizer.purge();
                Some(KeepUpInstruction::CatchUp)
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
                    Some(KeepUpInstruction::Do(
                        Duration::ZERO,
                        effect_builder.immediately().event(|_| {
                            MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                        }),
                    ))
                } else {
                    None
                }
            }
            SyncInstruction::CaughtUp { .. } => {
                // noop
                None
            }
        }
    }

    fn sync_back_process(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> KeepUpInstruction {
        let sync_back_progress = self.block_synchronizer.historical_progress();
        self.update_last_progress(&sync_back_progress, true);
        match self.sync_back_maybe_parent_block_identifier(&sync_back_progress) {
            Err(msg) => KeepUpInstruction::Fatal(msg),
            Ok(None) => KeepUpInstruction::CheckLater(
                format!("Historical: syncing {:?}", sync_back_progress),
                self.control_logic_default_delay.into(),
            ),
            Ok(Some((parent_hash, era_id))) => match self.validator_matrix.has_era(&era_id) {
                true => self.sync_back_register(effect_builder, rng, parent_hash),
                false => self.sync_back_leap(effect_builder, rng, parent_hash),
            },
        }
    }

    fn sync_back_leap(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
    ) -> KeepUpInstruction {
        debug!("Historical: sync leaping for: {}", parent_hash);
        let leap_status = self.sync_leaper.leap_status();
        debug!("Historical: {:?}", leap_status);
        match leap_status {
            LeapStatus::Idle => self.sync_back_leaper_idle(effect_builder, rng, parent_hash),
            LeapStatus::Failed { error, .. } => {
                self.sync_back_leap_failed(effect_builder, rng, parent_hash, error)
            }
            LeapStatus::Received {
                best_available,
                from_peers: _,
                ..
            } => self.sync_back_leap_received(best_available),
            LeapStatus::Awaiting { .. } => KeepUpInstruction::CheckLater(
                "Historical: sync leap awaiting".to_string(),
                self.control_logic_default_delay.into(),
            ),
        }
    }

    fn sync_back_leap_failed(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
        error: LeapActivityError,
    ) -> KeepUpInstruction {
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
        self.sync_back_leaper_idle(effect_builder, rng, parent_hash)
    }

    fn sync_back_leaper_idle(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
    ) -> KeepUpInstruction {
        let peers_to_ask = self.net.fully_connected_peers_random(
            rng,
            self.chainspec.core_config.simultaneous_peer_requests as usize,
        );
        let sync_leap_identifier = SyncLeapIdentifier::sync_to_historical(parent_hash);
        let effects = effect_builder.immediately().event(move |_| {
            MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                sync_leap_identifier,
                peers_to_ask,
            })
        });
        info!("Historical: initiating sync leap for: {}", parent_hash);
        KeepUpInstruction::Do(Duration::ZERO, effects)
    }

    fn sync_back_leap_received(&mut self, best_available: Box<SyncLeap>) -> KeepUpInstruction {
        info!(
            "Historical: sync leap received for: {:?}",
            best_available.trusted_block_header.block_hash()
        );
        let era_validator_weights =
            best_available.era_validator_weights(self.validator_matrix.fault_tolerance_threshold());
        for evw in era_validator_weights {
            let era_id = evw.era_id();
            if self.validator_matrix.register_era_validator_weights(evw) {
                info!("Historical: got era: {}", era_id);
            } else {
                debug!("Historical: already had era: {}", era_id);
            }
        }
        KeepUpInstruction::CheckLater("Historical: sync leap received".to_string(), Duration::ZERO)
    }

    fn sync_back_register(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
    ) -> KeepUpInstruction {
        if self.block_synchronizer.register_block_by_hash(
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

            KeepUpInstruction::Do(
                Duration::ZERO,
                effect_builder.immediately().event(|_| {
                    MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext)
                }),
            )
        } else {
            self.block_synchronizer.purge_historical();
            KeepUpInstruction::CheckLater(
                "Historical: purged".to_string(),
                self.control_logic_default_delay.into(),
            )
        }
    }

    fn sync_back_maybe_parent_block_identifier(
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
}
