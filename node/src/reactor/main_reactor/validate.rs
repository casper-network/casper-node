use std::time::Duration;
use tracing::{debug, info, warn};

use crate::{
    components::consensus::ChainspecConsensusExt,
    effect::{EffectBuilder, Effects},
    reactor,
    reactor::main_reactor::{MainEvent, MainReactor},
    storage::HighestOrphanedBlockResult,
    NodeRng,
};

pub(super) enum ValidateInstruction {
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    NonSwitchBlock,
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
        if self.switch_block_header.is_none() {
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

        if let Some(current_era) = self.consensus.current_era() {
            debug!(state = %self.state,
                era = current_era.value(),
                "{}: consensus current_era", self.state);
            if highest_switch_block_header.next_block_era_id() <= current_era {
                return Ok(Some(Effects::new()));
            }
        }

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

        if self.synced_to_ttl(Some(highest_switch_block_header))? {
            if let HighestOrphanedBlockResult::Orphan(header) =
                self.storage.get_highest_orphaned_block_header()
            {
                self.validator_matrix
                    .register_retrograde_latch(Some(header.era_id()));
            } else {
                return Err(
                    "get_highest_orphaned_block_header failed to produce record".to_string()
                );
            }
            debug!(%self.state,"{}: sufficient deploy TTL awareness to safely participate in consensus", self.state);
        } else {
            info!(
                "{}: insufficient deploy TTL awareness to safely participate in consensus",
                self.state
            );
            return Ok(None);
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
