use either::Either;
use std::{
    fmt::{Display, Formatter},
    time::Duration,
};
use tracing::{debug, error, info, warn};

use casper_types::{EraId, Timestamp};

use crate::{
    components::{
        block_accumulator::{SyncIdentifier, SyncInstruction},
        block_synchronizer::BlockSynchronizerProgress,
        sync_leaper,
        sync_leaper::{LeapActivityError, LeapStatus},
    },
    effect::{requests::BlockSynchronizerRequest, EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{ActivationPoint, BlockHash, Item, SyncLeap, SyncLeapIdentifier},
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

enum SyncBackInstruction {
    Sync {
        parent_hash: BlockHash,
        era_id: EraId,
    },
    Syncing,
    TtlSynced,
    GenesisSynced,
}

impl Display for SyncBackInstruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncBackInstruction::Sync {
                parent_hash: block_hash,
                ..
            } => {
                write!(f, "attempt to sync {}", block_hash)
            }
            SyncBackInstruction::Syncing => write!(f, "syncing"),
            SyncBackInstruction::TtlSynced => write!(f, "ttl reached"),
            SyncBackInstruction::GenesisSynced => write!(f, "genesis reached"),
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
            BlockSynchronizerProgress::Synced(block_hash, block_height, era_id) => {
                // for a synced forward block -> we have header, body, any referenced deploys,
                // and sufficient finality (by weight) of signatures. this node will ultimately
                // attempt to execute this block to produce global state and execution effects.
                Either::Left(self.keep_up_synced(block_hash, block_height, era_id))
            }
        }
    }

    fn keep_up_idle(&mut self) -> Either<SyncIdentifier, KeepUpInstruction> {
        match self.storage.read_highest_complete_block() {
            Ok(Some(block)) => Either::Left(SyncIdentifier::LocalTip(
                block.id(),
                block.height(),
                block.header().era_id(),
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
            SyncInstruction::Leap { .. } => {
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
                    .register_block_by_hash(block_hash, false, true)
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
        self.update_last_progress(&sync_back_progress, true);
        match self.sync_back_instruction(&sync_back_progress) {
            Ok(Some(sync_back_instruction)) => match sync_back_instruction {
                SyncBackInstruction::TtlSynced | SyncBackInstruction::GenesisSynced => {
                    // we don't need to sync any historical blocks currently
                    self.block_synchronizer.purge_historical();
                    None
                }
                SyncBackInstruction::Syncing => Some(KeepUpInstruction::CheckLater(
                    format!("historical {}", SyncBackInstruction::Syncing),
                    self.control_logic_default_delay.into(),
                )),
                SyncBackInstruction::Sync {
                    parent_hash,
                    era_id,
                } => match self.validator_matrix.has_era(&era_id) {
                    true => Some(self.sync_back_register(effect_builder, rng, parent_hash)),
                    false => Some(self.sync_back_leap(effect_builder, rng, parent_hash)),
                },
            },
            Ok(None) => None,
            Err(msg) => Some(KeepUpInstruction::Fatal(msg)),
        }
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
        let leap_status = self.sync_leaper.leap_status();
        info!(%parent_hash, %leap_status, "historical status");
        match leap_status {
            LeapStatus::Idle => {
                self.sync_back_leaper_idle(effect_builder, rng, parent_hash, Duration::ZERO)
            }
            LeapStatus::Awaiting { .. } => KeepUpInstruction::CheckLater(
                "historical sync leaper is awaiting response".to_string(),
                self.control_logic_default_delay.into(),
            ),
            LeapStatus::Received {
                best_available,
                from_peers: _,
                ..
            } => self.sync_back_leap_received(best_available),
            LeapStatus::Failed { error, .. } => {
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
        self.attempts += 1;
        warn!(
            %error,
            remaining_attempts = %self.max_attempts.saturating_sub(self.attempts),
            "historical: failed leap",
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
        let sync_leap_identifier = SyncLeapIdentifier::sync_to_historical(parent_hash);
        let effects = effect_builder.immediately().event(move |_| {
            MainEvent::SyncLeaper(sync_leaper::Event::AttemptLeap {
                sync_leap_identifier,
                peers_to_ask,
            })
        });
        KeepUpInstruction::Do(offset, effects)
    }

    fn sync_back_leap_received(&mut self, best_available: Box<SyncLeap>) -> KeepUpInstruction {
        // use the leap response to update our recent switch block data (if relevant) and
        // era validator weights. if there are other processes which are holding on discovery
        // of relevant newly-seen era validator weights, they should naturally progress
        // themselves via notification on the event loop.
        if let Err(msg) = self.update_highest_switch_block() {
            return KeepUpInstruction::Fatal(msg);
        }
        let block_hash = best_available.highest_block_hash();
        let block_height = best_available.highest_block_height();
        info!(
            %best_available, %block_height, %block_hash, "historical: leap received");

        let era_validator_weights =
            best_available.era_validator_weights(self.validator_matrix.fault_tolerance_threshold());
        for evw in era_validator_weights {
            let era_id = evw.era_id();
            if self.validator_matrix.register_era_validator_weights(evw) {
                info!(%era_id, "historical: got era");
            } else {
                debug!(%era_id, "historical: era already present or is not relevant");
            }
        }
        KeepUpInstruction::CheckLater("historical sync leap received".to_string(), Duration::ZERO)
    }

    fn sync_back_register(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        parent_hash: BlockHash,
    ) -> KeepUpInstruction {
        if self
            .block_synchronizer
            .register_block_by_hash(parent_hash, true, true)
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
                "historical: register_block_by_hash: {} peers count: {:?}",
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
        if matches!(
            block_synchronizer_progress,
            BlockSynchronizerProgress::Syncing(_, _, _)
        ) {
            return Ok(Some(SyncBackInstruction::Syncing));
        }
        // in this flow there is no significant difference between Idle & Synced, as unlike in
        // catchup and keepup flows there is no special activity necessary upon getting to Synced
        // on an old block. in either case we will attempt to get the next needed block (if any).
        // note: for a synced historical block we have header, body, global state, any execution
        // effects, any referenced deploys, & sufficient finality (by weight) of signatures.
        match self.storage.get_highest_orphaned_block_header() {
            Some(block_header) => {
                if block_header.is_genesis() {
                    return Ok(Some(SyncBackInstruction::GenesisSynced));
                }
                if self.sync_back_is_ttl() {
                    return Ok(Some(SyncBackInstruction::TtlSynced));
                }
                let parent_hash = block_header.parent_hash();
                match self.storage.read_block_header(parent_hash) {
                    Ok(Some(parent_block_header)) => {
                        // even if we don't have a complete block (all parts and dependencies)
                        // we may have the parent's block header; if we do we also
                        // know its era which allows us to know if we have the validator
                        // set for that era or not
                        Ok(Some(SyncBackInstruction::Sync {
                            parent_hash: parent_block_header.block_hash(),
                            era_id: parent_block_header.era_id(),
                        }))
                    }
                    Ok(None) => match block_header.era_id().predecessor() {
                        // we do not have the parent header and thus don't know what era
                        // the parent block is in (it could be the same era or the previous era).
                        // we assume the worst case and ask for the earlier era's proof
                        Some(previous_era_id) => Ok(Some(SyncBackInstruction::Sync {
                            parent_hash: *parent_hash,
                            era_id: previous_era_id,
                        })),
                        None => Ok(None),
                    },
                    Err(err) => Err(err.to_string()),
                }
            }
            None => Ok(None),
        }
    }

    fn sync_back_is_ttl(&self) -> bool {
        if false == self.sync_to_genesis {
            // if sync to genesis is false, we require sync to ttl; i.e. if the TTL is 12 hours
            // we require sync back to see a contiguous / unbroken range of at least 12 hours
            // worth of blocks. note however that we measure from the start of the active era
            // (for consensus reasons), so this can be up to TTL + era length in practice
            if let Some(block_header) = &self.switch_block {
                let diff = self.chainspec.deploy_config.max_ttl;
                let cutoff = block_header.timestamp().saturating_sub(diff);
                let block_time = block_header.timestamp();
                // this node is configured to only sync to ttl, and we may have reached ttl
                return block_time < cutoff;
            }
        }
        false
    }
}
