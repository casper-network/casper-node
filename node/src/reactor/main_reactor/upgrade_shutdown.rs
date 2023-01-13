use std::{collections::HashMap, time::Duration};

use datasize::DataSize;
use tracing::debug;

use crate::{
    effect::{announcements::ControlAnnouncement, EffectBuilder, EffectExt, Effects},
    reactor::main_reactor::{MainEvent, MainReactor},
    types::{BlockHash, EraValidatorWeights, FinalitySignatureId},
};

use casper_types::EraId;

const DELAY_BEFORE_SHUTDOWN: Duration = Duration::from_secs(2);

#[derive(Debug, DataSize)]
pub(super) struct SignatureGossipTracker {
    era_id: EraId,
    finished_gossiping: HashMap<BlockHash, Vec<FinalitySignatureId>>,
}

impl SignatureGossipTracker {
    pub(super) fn new() -> Self {
        Self {
            era_id: EraId::from(0),
            finished_gossiping: Default::default(),
        }
    }

    pub(super) fn register_signature(&mut self, signature_id: FinalitySignatureId) {
        // ignore the signature if it's from an older era
        if signature_id.era_id < self.era_id {
            return;
        }
        // if we registered a signature in a higher era, reset the cache
        if signature_id.era_id > self.era_id {
            self.era_id = signature_id.era_id;
            self.finished_gossiping = Default::default();
        }
        // record that the signature has finished gossiping
        self.finished_gossiping
            .entry(signature_id.block_hash)
            .or_default()
            .push(signature_id);
    }

    fn finished_gossiping_enough(&self, validator_weights: &EraValidatorWeights) -> bool {
        if validator_weights.era_id() != self.era_id {
            debug!(
                relevant_era=%validator_weights.era_id(),
                our_era_id=%self.era_id,
                "SignatureGossipTracker has no record of the relevant era!"
            );
            return false;
        }
        self.finished_gossiping
            .iter()
            .all(|(block_hash, signatures)| {
                let gossiped_weight_sufficient = validator_weights
                    .signature_weight(signatures.iter().map(|sig_id| &sig_id.public_key))
                    .is_sufficient(true);
                debug!(
                    %gossiped_weight_sufficient,
                    %block_hash,
                    "SignatureGossipTracker: gossiped finality signatures check"
                );
                gossiped_weight_sufficient
            })
    }
}

pub(super) enum UpgradeShutdownInstruction {
    Do(Duration, Effects<MainEvent>),
    CheckLater(String, Duration),
    Fatal(String),
}

impl MainReactor {
    pub(super) fn upgrade_shutdown_instruction(
        &self,
        effect_builder: EffectBuilder<MainEvent>,
    ) -> UpgradeShutdownInstruction {
        let recent_switch_block_headers = match self.storage.read_highest_switch_block_headers(1) {
            Ok(headers) => headers,
            Err(error) => {
                return UpgradeShutdownInstruction::Fatal(format!(
                    "error getting recent switch block headers: {}",
                    error
                ))
            }
        };
        if let Some(block_header) = recent_switch_block_headers.last() {
            let highest_switch_block_era = block_header.era_id();
            return match self
                .validator_matrix
                .validator_weights(highest_switch_block_era)
            {
                Some(validator_weights) => self
                    .upgrade_shutdown_has_sufficient_finality(effect_builder, &validator_weights),
                None => UpgradeShutdownInstruction::Fatal(
                    "validator_weights cannot be missing".to_string(),
                ),
            };
        }
        UpgradeShutdownInstruction::Fatal("recent_switch_block_headers cannot be empty".to_string())
    }

    fn upgrade_shutdown_has_sufficient_finality(
        &self,
        effect_builder: EffectBuilder<MainEvent>,
        validator_weights: &EraValidatorWeights,
    ) -> UpgradeShutdownInstruction {
        let finished_gossiping_enough = self
            .signature_gossip_tracker
            .finished_gossiping_enough(validator_weights);
        match self
            .storage
            .era_has_sufficient_finality_signatures(validator_weights)
        {
            Ok(true) if finished_gossiping_enough => {
                // Allow a delay to acquire more finality signatures
                let effects = effect_builder
                    .set_timeout(DELAY_BEFORE_SHUTDOWN)
                    .event(|_| {
                        MainEvent::ControlAnnouncement(ControlAnnouncement::ShutdownForUpgrade)
                    });
                // should not need to crank the control logic again as the reactor will shutdown
                UpgradeShutdownInstruction::Do(DELAY_BEFORE_SHUTDOWN, effects)
            }
            Ok(_) => UpgradeShutdownInstruction::CheckLater(
                "waiting for sufficient finality and completion of gossiping signatures"
                    .to_string(),
                DELAY_BEFORE_SHUTDOWN,
            ),
            Err(error) => UpgradeShutdownInstruction::Fatal(format!(
                "failed check for sufficient finality signatures: {}",
                error
            )),
        }
    }
}
