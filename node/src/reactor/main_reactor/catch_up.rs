use either::Either;
use std::time::Duration;
use tracing::{debug, info, warn};

use casper_types::{TimeDiff, Timestamp};

use crate::{
    components::{
        block_accumulator::{SyncIdentifier, SyncInstruction},
        block_synchronizer::BlockSynchronizerProgress,
        sync_leaper,
        sync_leaper::{LeapActivityError, LeapStatus},
        ValidatorBoundComponent,
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{ActivationPoint, BlockHash, NodeId, SyncLeap, SyncLeapIdentifier},
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
        // if there is instruction, return to start working on it
        // else fall thru with the current best available id for block syncing
        let sync_identifier = match self.catch_up_process() {
            Either::Right(catch_up_instruction) => return catch_up_instruction,
            Either::Left(sync_identifier) => sync_identifier,
        };
        debug!(
            ?sync_identifier,
            block_hash = %sync_identifier.block_hash(),
            "CatchUp: sync identifier"
        );
        // we check with the block accumulator before doing sync work as it may be aware of one or
        // more blocks that are higher than our current highest block
        let sync_instruction = self.block_accumulator.sync_instruction(sync_identifier);
        debug!(
            ?sync_instruction,
            block_hash = %sync_instruction.block_hash(),
            "CatchUp: sync_instruction"
        );
        if let Some(catch_up_instruction) =
            self.catch_up_sync_instruction(effect_builder, rng, sync_instruction)
        {
            // do necessary work to catch up
            return catch_up_instruction;
        }
        // there are no catch up or shutdown instructions, so we must be caught up
        CatchUpInstruction::CaughtUp
    }

    fn catch_up_process(&mut self) -> Either<SyncIdentifier, CatchUpInstruction> {
        let catch_up_progress = self.block_synchronizer.historical_progress();
        self.update_last_progress(&catch_up_progress, false);
        match catch_up_progress {
            BlockSynchronizerProgress::Idle => {
                // not working on syncing a block (ready to start a new one)
                match self.trusted_hash {
                    Some(trusted_hash) => self.catch_up_trusted_hash(trusted_hash),
                    None => self.catch_up_no_trusted_hash(),
                }
            }
            BlockSynchronizerProgress::Syncing(block_hash, maybe_block_height, last_progress) => {
                // working on syncing a block
                self.catch_up_syncing(block_hash, maybe_block_height, last_progress)
            }
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => Either::Left(
                // for a synced CatchUp block -> we have header, body, global state, any execution
                // effects, any referenced deploys, & sufficient finality (by weight) of signatures
                SyncIdentifier::SyncedBlockIdentifier(block_hash, block_height, era_id),
            ),
        }
    }

    fn catch_up_no_trusted_hash(&mut self) -> Either<SyncIdentifier, CatchUpInstruction> {
        // no trusted hash provided, we will attempt to use local tip if available
        match self.storage.read_highest_complete_block() {
            Ok(Some(block)) => {
                // this is typically a restart scenario; if a node stops and restarts
                // quickly enough they can rejoin the network from their highest local block
                // if too much time has passed, the node will shutdown and require a
                // trusted block hash to be provided via the config file
                info!("CatchUp: local tip detected, no trusted hash");
                if block.header().is_switch_block() {
                    self.switch_block = Some(block.header().clone());
                }
                Either::Left(SyncIdentifier::LocalTip(
                    *block.hash(),
                    block.height(),
                    block.header().era_id(),
                ))
            }
            Ok(None) if self.switch_block.is_none() => {
                // no trusted hash, no local block, might be genesis
                self.catch_up_check_genesis()
            }
            Ok(None) => {
                // no trusted hash, no local block, no error, must be waiting for genesis
                info!("CatchUp: waiting to store genesis immediate switch block");
                Either::Right(CatchUpInstruction::CheckLater(
                    "waiting for genesis immediate switch block to be stored".to_string(),
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

    fn catch_up_check_genesis(&mut self) -> Either<SyncIdentifier, CatchUpInstruction> {
        match self.chainspec.protocol_config.activation_point {
            ActivationPoint::Genesis(timestamp) => {
                // this bootstraps a network; it only occurs once ever on a given network but is
                // very load-bearing as errors in this logic can prevent the network from coming
                // into existence or surviving its initial existence.

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
                        format!("waiting for genesis activation at {}", timestamp),
                        Duration::from(time_remaining),
                    ));
                }
                Either::Right(CatchUpInstruction::CommitGenesis)
            }
            ActivationPoint::EraId(_) => {
                // no trusted hash, no local block, not genesis
                Either::Right(CatchUpInstruction::Fatal(
                    "CatchUp: cannot proceed without trusted hash".to_string(),
                ))
            }
        }
    }

    fn catch_up_trusted_hash(
        &mut self,
        trusted_hash: BlockHash,
    ) -> Either<SyncIdentifier, CatchUpInstruction> {
        // if we have a configured trusted hash and we have the header for that block,
        // use the higher block height of the local tip and the trusted header
        match self.storage.read_block_header(&trusted_hash) {
            Ok(Some(trusted_header)) => {
                match self.storage.read_highest_complete_block() {
                    Ok(Some(block)) => {
                        // leap w/ the higher of local tip or trusted hash
                        let trusted_height = trusted_header.height();
                        if trusted_height > block.height() {
                            Either::Left(SyncIdentifier::BlockIdentifier(
                                trusted_hash,
                                trusted_height,
                            ))
                        } else {
                            Either::Left(SyncIdentifier::LocalTip(
                                *block.hash(),
                                block.height(),
                                block.header().era_id(),
                            ))
                        }
                    }
                    Ok(None) => Either::Left(SyncIdentifier::BlockHash(trusted_hash)),
                    Err(_) => Either::Right(CatchUpInstruction::Fatal(
                        "CatchUp: fatal block store error when attempting to \
                                            read highest complete block"
                            .to_string(),
                    )),
                }
            }
            Ok(None) => {
                // we do not have the header for the trusted hash. we may have local tip,
                // but we start with the configured trusted hash in this scenario as it is
                // necessary to allow a node to re-join if their local state is stale
                Either::Left(SyncIdentifier::BlockHash(trusted_hash))
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
    ) -> Either<SyncIdentifier, CatchUpInstruction> {
        // if any progress has been made, reset attempts
        if last_progress > self.last_progress {
            debug!(%last_progress, "CatchUp: syncing");
            self.last_progress = last_progress;
            self.attempts = 0;
        }
        // if we have not made progress on our attempt to catch up with the network, increment
        // attempts counter and try again; the crank logic will shut the node down on the next
        // crank if we've exceeded our reattempts
        let idleness = Timestamp::now().saturating_diff(last_progress);
        if idleness > self.idle_tolerance {
            self.attempts += 1;
            warn!(
                %last_progress,
                remaining_attempts = self.max_attempts.saturating_sub(self.attempts),
                "CatchUp: idleness detected"
            );
        }
        match maybe_block_height {
            None => Either::Left(SyncIdentifier::BlockHash(block_hash)),
            Some(block_height) => {
                Either::Left(SyncIdentifier::BlockIdentifier(block_hash, block_height))
            }
        }
    }

    fn catch_up_sync_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        sync_instruction: SyncInstruction,
    ) -> Option<CatchUpInstruction> {
        match sync_instruction {
            SyncInstruction::Leap { block_hash } => {
                Some(self.catch_up_leap(effect_builder, rng, block_hash))
            }
            SyncInstruction::BlockSync { block_hash } => {
                Some(self.catch_up_block_sync(effect_builder, block_hash))
            }
            SyncInstruction::CaughtUp { .. } => self.catch_up_check_transition(),
        }
    }

    fn catch_up_leap(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> CatchUpInstruction {
        // register block builder so that control logic can tell that block is Syncing,
        // otherwise block_synchronizer detects as Idle which can cause unnecessary churn
        // on subsequent cranks while leaper is awaiting responses.
        self.block_synchronizer
            .register_block_by_hash(block_hash, true, true);
        let leap_status = self.sync_leaper.leap_status();
        info!(%block_hash, %leap_status, "CatchUp: status");
        match leap_status {
            LeapStatus::Idle => self.catch_up_leaper_idle(effect_builder, rng, block_hash),
            LeapStatus::Awaiting { .. } => CatchUpInstruction::CheckLater(
                "sync leaper is awaiting response".to_string(),
                self.control_logic_default_delay.into(),
            ),
            LeapStatus::Received {
                best_available,
                from_peers,
                ..
            } => self.catch_up_leap_received(effect_builder, best_available, from_peers),
            LeapStatus::Failed { error, .. } => {
                self.catch_up_leap_failed(effect_builder, rng, block_hash, error)
            }
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
        warn!(
            %error,
            remaining_attempts = %self.max_attempts.saturating_sub(self.attempts),
            "CatchUp: failed leap",
        );
        self.catch_up_leaper_idle(effect_builder, rng, block_hash)
    }

    fn catch_up_leaper_idle(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        block_hash: BlockHash,
    ) -> CatchUpInstruction {
        // we get a random sampling of peers to ask.
        let peers_to_ask = self.net.fully_connected_peers_random(
            rng,
            self.chainspec.core_config.simultaneous_peer_requests as usize,
        );
        if peers_to_ask.is_empty() {
            return CatchUpInstruction::CheckLater(
                "no peers".to_string(),
                self.chainspec.core_config.minimum_block_time.into(),
            );
        }
        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(block_hash);
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
        let block_hash = best_available.highest_block_hash();
        let block_height = best_available.highest_block_height();
        info!(
            %best_available,
            %block_height,
            %block_hash,
            "CatchUp: leap received"
        );

        if let Err(msg) = self.update_highest_switch_block() {
            return CatchUpInstruction::Fatal(msg);
        }

        for validator_weights in
            best_available.era_validator_weights(self.validator_matrix.fault_tolerance_threshold())
        {
            self.validator_matrix
                .register_era_validator_weights(validator_weights);
        }

        self.block_synchronizer
            .register_sync_leap(&*best_available, from_peers, true);
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
        if self
            .block_synchronizer
            .register_block_by_hash(block_hash, true, true)
        {
            // NeedNext will self perpetuate until nothing is needed for this block
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

    fn catch_up_check_transition(&mut self) -> Option<CatchUpInstruction> {
        // we may be starting back up after a shutdown for upgrade; if so we need to
        // commit upgrade now before proceeding further
        if self.should_commit_upgrade() {
            return Some(CatchUpInstruction::CommitUpgrade);
        }
        // we may need to shutdown to go thru an upgrade
        if self.should_shutdown_for_upgrade() {
            Some(CatchUpInstruction::ShutdownForUpgrade)
        } else {
            None
        }
    }
}
