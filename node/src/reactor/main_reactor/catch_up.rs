use either::Either;
use std::time::Duration;
use tracing::{debug, info, trace};

use casper_types::{TimeDiff, Timestamp};

use crate::{
    components::{
        block_accumulator::{StartingWith, SyncInstruction},
        block_synchronizer::BlockSynchronizerProgress,
        sync_leaper,
        sync_leaper::{LeapActivityError, LeapStatus},
        ValidatorBoundComponent,
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{ActivationPoint, BlockHash, NodeId, SyncLeap, SyncLeapIdentifier},
    utils::DisplayIter,
    NodeRng,
};

pub(super) enum CatchUpInstruction {
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    Fatal(String),
    ShutdownForUpgrade,
    CaughtUp,
    CommitGenesis,
    CommitUpgrade,
}

impl MainReactor {
    pub(super) fn catch_up_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> CatchUpInstruction {
        let starting_with = match self.catch_up_process() {
            Either::Left(starting_with) => starting_with,
            Either::Right(instruction) => return instruction,
        };
        debug!("CatchUp: starting with {:?}", starting_with);
        let sync_instruction = self.block_accumulator.sync_instruction(starting_with);
        debug!(
            ?sync_instruction,
            "CatchUp: sync_instruction {}",
            sync_instruction.block_hash()
        );
        match sync_instruction {
            SyncInstruction::Leap { block_hash } => {
                return self.catch_up_leap(effect_builder, rng, block_hash);
            }
            SyncInstruction::BlockSync { block_hash } => {
                return self.catch_up_block_sync(effect_builder, block_hash);
            }
            SyncInstruction::CaughtUp { .. } => {
                if let Some(instruction) = self.catch_up_caught_up() {
                    return instruction;
                }
            }
        }
        // purge synchronizer to keep state detection and reporting correct and fresh
        self.block_synchronizer.purge();
        // there are no catch up or shutdown instructions, so we must be caught up
        CatchUpInstruction::CaughtUp
    }

    fn catch_up_process(&mut self) -> Either<StartingWith, CatchUpInstruction> {
        let catch_up_progress = self.block_synchronizer.historical_progress();
        self.update_last_progress(&catch_up_progress, false);
        match catch_up_progress {
            BlockSynchronizerProgress::Idle => match self.trusted_hash {
                None => self.catch_up_no_trusted_hash(),
                Some(trusted_hash) => self.catch_up_trusted_hash(trusted_hash),
            },
            BlockSynchronizerProgress::Syncing(block_hash, maybe_block_height, last_progress) => {
                self.catch_up_syncing(block_hash, maybe_block_height, last_progress)
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => Either::Left(
                StartingWith::SyncedBlockIdentifier(block_hash, block_height, era_id),
            ),
        }
    }

    fn catch_up_no_trusted_hash(&mut self) -> Either<StartingWith, CatchUpInstruction> {
        // no trusted hash provided use local tip if available
        match self.storage.read_highest_complete_block() {
            Ok(Some(block)) => {
                // -+ : leap w/ local tip
                info!("CatchUp: local tip detected, no trusted hash");
                if block.header().is_switch_block() {
                    self.switch_block = Some(block.header().clone());
                }
                Either::Left(StartingWith::LocalTip(
                    *block.hash(),
                    block.height(),
                    block.header().era_id(),
                ))
            }
            Ok(None) if self.switch_block.is_none() => {
                if let Some(timestamp) = self.is_genesis() {
                    let is_validator = self.should_commit_genesis();
                    if false == is_validator {
                        return Either::Right(CatchUpInstruction::Fatal(
                            "CatchUp: only validating nodes may participate in genesis; cannot proceed without trusted hash".to_string(),
                        ));
                    }
                    let now = Timestamp::now();
                    let grace_period = timestamp.saturating_add(TimeDiff::from_seconds(180));
                    if now > grace_period {
                        return Either::Right(CatchUpInstruction::Fatal(
                            "CatchUp: late for genesis; cannot proceed without trusted hash"
                                .to_string(),
                        ));
                    }
                    let time_remaining = timestamp.saturating_diff(now);
                    if time_remaining > TimeDiff::default() {
                        return Either::Right(CatchUpInstruction::CheckLater(
                            format!("CatchUp: waiting for genesis activation at {}", timestamp),
                            Duration::from(time_remaining),
                        ));
                    }
                    return Either::Right(CatchUpInstruction::CommitGenesis);
                }
                // -- : no trusted hash, no local block, not genesis
                Either::Right(CatchUpInstruction::Fatal(
                    "CatchUp: cannot proceed without trusted hash".to_string(),
                ))
            }
            Ok(None) => {
                debug!("CatchUp: waiting to store genesis immediate switch block");
                Either::Right(CatchUpInstruction::CheckLater(
                    "CatchUp: waiting for genesis immediate switch block to be stored".to_string(),
                    self.control_logic_default_delay.into(),
                ))
            }
            Err(err) => Either::Right(CatchUpInstruction::Fatal(format!(
                "CatchUp: fatal block store error when attempting to read \
                                    highest complete block: {}",
                err
            ))),
        }
    }

    fn catch_up_trusted_hash(
        &mut self,
        trusted_hash: BlockHash,
    ) -> Either<StartingWith, CatchUpInstruction> {
        // if we have a trusted hash and we have a local tip, use the higher
        match self.storage.read_block_header(&trusted_hash) {
            Ok(Some(trusted_header)) => {
                match self.storage.read_highest_complete_block() {
                    Ok(Some(block)) => {
                        // ++ : leap w/ the higher of local tip or trusted hash
                        let trusted_height = trusted_header.height();
                        if trusted_height > block.height() {
                            Either::Left(StartingWith::BlockIdentifier(
                                trusted_hash,
                                trusted_height,
                            ))
                        } else {
                            Either::Left(StartingWith::LocalTip(
                                *block.hash(),
                                block.height(),
                                block.header().era_id(),
                            ))
                        }
                    }
                    Ok(None) => {
                        // should be unreachable if we've gotten this far
                        Either::Left(StartingWith::Hash(trusted_hash))
                    }
                    Err(_) => Either::Right(CatchUpInstruction::Fatal(
                        "CatchUp: fatal block store error when attempting to \
                                            read highest complete block"
                            .to_string(),
                    )),
                }
            }
            Ok(None) => {
                // +- : leap w/ config hash
                Either::Left(StartingWith::Hash(trusted_hash))
            }
            Err(err) => Either::Right(CatchUpInstruction::Fatal(format!(
                "CatchUp: fatal block store error when attempting to read \
                                    highest complete block: {}",
                err
            ))),
        }
    }

    fn catch_up_syncing(
        &mut self,
        block_hash: BlockHash,
        maybe_block_height: Option<u64>,
        last_progress: Timestamp,
    ) -> Either<StartingWith, CatchUpInstruction> {
        // do idleness / reattempt checking
        if Timestamp::now().saturating_diff(last_progress) > self.idle_tolerance {
            self.attempts += 1;
            if self.attempts > self.max_attempts {
                return Either::Right(CatchUpInstruction::Fatal(
                    "CatchUp: block sync idleness exceeded reattempt tolerance".to_string(),
                ));
            }
        }
        // if any progress has been made, reset attempts
        if last_progress > self.last_progress {
            debug!("CatchUp: syncing last_progress: {}", last_progress);
            self.last_progress = last_progress;
            self.attempts = 0;
        }
        match maybe_block_height {
            None => Either::Left(StartingWith::Hash(block_hash)),
            Some(block_height) => {
                Either::Left(StartingWith::BlockIdentifier(block_hash, block_height))
            }
        }
    }

    fn catch_up_leap(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> CatchUpInstruction {
        // register block builder so that control logic can tell that block is Syncing,
        // otherwise block_synchronizer detects as Idle
        self.block_synchronizer.register_block_by_hash(
            block_hash,
            true,
            true,
            self.chainspec.core_config.simultaneous_peer_requests,
        );
        let leap_status = self.sync_leaper.leap_status();
        trace!("CatchUp: leap_status: {:?}", leap_status);
        match leap_status {
            LeapStatus::Idle => self.catch_up_leaper_idle(effect_builder, rng, block_hash),
            LeapStatus::Failed { error, .. } => {
                self.catch_up_leap_failed(effect_builder, rng, block_hash, error)
            }
            LeapStatus::Awaiting { .. } => CatchUpInstruction::CheckLater(
                "sync leaper is awaiting response".to_string(),
                self.control_logic_default_delay.into(),
            ),
            LeapStatus::Received {
                best_available,
                from_peers,
                ..
            } => self.catch_up_leap_received(effect_builder, best_available, from_peers),
        }
    }

    fn catch_up_leap_failed(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        block_hash: BlockHash,
        error: LeapActivityError,
    ) -> CatchUpInstruction {
        self.attempts += 1;
        if self.attempts > self.max_attempts {
            CatchUpInstruction::Fatal(format!(
                "CatchUp: failed leap exceeded reattempt tolerance: {}",
                error,
            ))
        } else {
            self.catch_up_leaper_idle(effect_builder, rng, block_hash)
        }
    }

    fn catch_up_leaper_idle(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> CatchUpInstruction {
        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(block_hash);
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

    fn catch_up_leap_received(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        best_available: Box<SyncLeap>,
        from_peers: Vec<NodeId>,
    ) -> CatchUpInstruction {
        debug!(
            "CatchUp: {} received from {}",
            best_available,
            DisplayIter::new(&from_peers)
        );
        info!("CatchUp: {}", best_available);
        for validator_weights in
            best_available.era_validator_weights(self.validator_matrix.fault_tolerance_threshold())
        {
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
        let effects = effect_builder
            .immediately()
            .event(|_| MainEvent::BlockSynchronizerRequest(BlockSynchronizerRequest::NeedNext));
        CatchUpInstruction::Do(self.control_logic_default_delay.into(), effects)
    }

    fn catch_up_block_sync(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        block_hash: BlockHash,
    ) -> CatchUpInstruction {
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
            CatchUpInstruction::Do(Duration::ZERO, effects)
        } else {
            CatchUpInstruction::CheckLater(
                format!("block_synchronizer unable to register block {}", block_hash),
                self.control_logic_default_delay.into(),
            )
        }
    }

    fn catch_up_caught_up(&mut self) -> Option<CatchUpInstruction> {
        match self.should_commit_upgrade() {
            Err(msg) => return Some(CatchUpInstruction::Fatal(msg)),
            Ok(true) => return Some(CatchUpInstruction::CommitUpgrade),
            Ok(false) => (),
        }
        if self.should_shutdown_for_upgrade() {
            Some(CatchUpInstruction::ShutdownForUpgrade)
        } else {
            None
        }
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
}
