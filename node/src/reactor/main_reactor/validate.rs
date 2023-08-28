use std::time::Duration;
use tracing::{debug, info, warn};

use crate::{
    components::{
        block_accumulator::{SyncIdentifier, SyncInstruction},
        consensus::ChainspecConsensusExt,
    },
    effect::{EffectBuilder, Effects},
    reactor::{
        self,
        main_reactor::{MainEvent, MainReactor},
    },
    storage::HighestOrphanedBlockResult,
    types::MaxTtl,
    NodeRng,
};

/// Cranking delay when encountered a non-switch block when checking the validator status.
const VALIDATION_STATUS_DELAY_FOR_NON_SWITCH_BLOCK: Duration = Duration::from_secs(2);

pub(super) enum ValidateInstruction {
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    CatchUp,
    KeepUp,
    ShutdownForUpgrade,
    Fatal(String),
}

impl MainReactor {
    pub(super) fn validate_instruction(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> ValidateInstruction {
        let queue_depth = self.contract_runtime.queue_depth();
        if queue_depth > 0 {
            warn!("Validate: should_validate queue_depth {}", queue_depth);
            return ValidateInstruction::CheckLater(
                "allow time for contract runtime execution to occur".to_string(),
                self.control_logic_default_delay.into(),
            );
        }

        match self.storage.read_highest_complete_block() {
            Ok(Some(highest_complete_block)) => {
                // If we're lagging behind the rest of the network, fall back out of Validate mode.
                let sync_identifier = SyncIdentifier::LocalTip(
                    *highest_complete_block.hash(),
                    highest_complete_block.height(),
                    highest_complete_block.era_id(),
                );

                if let SyncInstruction::Leap { .. } =
                    self.block_accumulator.sync_instruction(sync_identifier)
                {
                    return ValidateInstruction::CatchUp;
                }

                if !highest_complete_block.is_switch_block() {
                    return ValidateInstruction::CheckLater(
                        "tip is not a switch block, don't change from validate state".to_string(),
                        VALIDATION_STATUS_DELAY_FOR_NON_SWITCH_BLOCK,
                    );
                }
            }
            Ok(None) => {
                return ValidateInstruction::CheckLater(
                    "no complete block found in storage".to_string(),
                    self.control_logic_default_delay.into(),
                );
            }
            Err(error) => {
                return ValidateInstruction::Fatal(format!(
                    "Could not read highest complete block from storage due to storage error: {}",
                    error
                ));
            }
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

    pub(super) fn create_required_eras(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
    ) -> Result<Option<Effects<MainEvent>>, String> {
        let recent_switch_block_headers = self
            .storage
            .read_highest_switch_block_headers(self.chainspec.number_of_past_switch_blocks_needed())
            .map_err(|err| err.to_string())?;

        let highest_switch_block_header = match recent_switch_block_headers.last() {
            None => {
                debug!(
                    "{}: create_required_eras: recent_switch_block_headers is empty",
                    self.state
                );
                return Ok(None);
            }
            Some(header) => header,
        };
        debug!(
            era = highest_switch_block_header.era_id().value(),
            block_hash = %highest_switch_block_header.block_hash(),
            height = highest_switch_block_header.height(),
            "{}: highest_switch_block_header", self.state
        );

        let highest_era_weights = match highest_switch_block_header.next_era_validator_weights() {
            None => {
                return Err(format!(
                    "{}: highest switch block has no era end: {}",
                    self.state, highest_switch_block_header,
                ));
            }
            Some(weights) => weights,
        };
        if !highest_era_weights.contains_key(self.consensus.public_key()) {
            info!(
                "{}: highest_era_weights does not contain signing_public_key",
                self.state
            );
            return Ok(None);
        }

        if let HighestOrphanedBlockResult::Orphan(highest_orphaned_block_header) =
            self.storage.get_highest_orphaned_block_header()
        {
            let max_ttl: MaxTtl = self.chainspec.deploy_config.max_ttl.into();
            if max_ttl.synced_to_ttl(
                highest_switch_block_header.timestamp(),
                &highest_orphaned_block_header,
            ) {
                debug!(%self.state,"{}: sufficient deploy TTL awareness to safely participate in consensus", self.state);
            } else {
                info!(
                    "{}: insufficient deploy TTL awareness to safely participate in consensus",
                    self.state
                );
                return Ok(None);
            }
        } else {
            return Err("get_highest_orphaned_block_header failed to produce record".to_string());
        }

        let era_id = highest_switch_block_header.era_id();
        if self.upgrade_watcher.should_upgrade_after(era_id) {
            info!(
                "{}: upgrade required after era {}",
                self.state,
                era_id.value()
            );
            return Ok(None);
        }

        let create_required_eras =
            self.consensus
                .create_required_eras(effect_builder, rng, &recent_switch_block_headers);
        match &create_required_eras {
            Some(effects) => {
                if effects.is_empty() {
                    info!(state = %self.state,"create_required_eras is empty");
                } else {
                    info!(state = %self.state,"will attempt to create required eras for consensus");
                }
            }
            None => {
                info!(state = %self.state,"create_required_eras is none");
            }
        }
        Ok(
            create_required_eras
                .map(|effects| reactor::wrap_effects(MainEvent::Consensus, effects)),
        )
    }
}
