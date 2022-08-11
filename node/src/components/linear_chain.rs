mod error;
mod event;
mod metrics;
mod pending_signatures;
mod signature;
mod signature_cache;
mod state;

use std::{collections::BTreeMap, convert::Infallible};

use datasize::DataSize;
use itertools::Itertools;
use num::rational::Ratio;
use prometheus::Registry;
use tracing::{error, info};

use casper_types::{EraId, ProtocolVersion, PublicKey, U512};

use self::{
    metrics::Metrics,
    state::{LinearChain, Outcome, Outcomes},
};
use crate::{
    components::{contract_runtime::EraValidatorsRequest, Component},
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, EffectResultExt, Effects,
    },
    protocol::Message,
    types::{ActivationPoint, BlockHeader},
    NodeRng,
};
pub(crate) use error::Error;
pub(crate) use event::Event;

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainComponent {
    linear_chain_state: LinearChain,
    #[data_size(skip)]
    metrics: Metrics,
    /// If true, the process should stop execution to allow an upgrade to proceed.
    stop_for_upgrade: bool,
}

impl LinearChainComponent {
    pub(crate) fn new(
        registry: &Registry,
        protocol_version: ProtocolVersion,
        auction_delay: u64,
        unbonding_delay: u64,
        finality_threshold_fraction: Ratio<u64>,
        next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> Result<Self, prometheus::Error> {
        let metrics = Metrics::new(registry)?;
        let linear_chain_state = LinearChain::new(
            protocol_version,
            auction_delay,
            unbonding_delay,
            finality_threshold_fraction,
            next_upgrade_activation_point,
        );
        Ok(LinearChainComponent {
            linear_chain_state,
            metrics,
            stop_for_upgrade: false,
        })
    }

    pub(crate) fn stop_for_upgrade(&self) -> bool {
        self.stop_for_upgrade
    }
}

fn outcomes_to_effects<REv>(
    effect_builder: EffectBuilder<REv>,
    outcomes: Outcomes,
) -> Effects<Event>
where
    REv: From<StorageRequest>
        + From<NetworkRequest<Message>>
        + From<LinearChainAnnouncement>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + Send,
{
    outcomes
        .into_iter()
        .map(|outcome| match outcome {
            Outcome::StoreBlockSignatures(block_signatures, should_upgrade) => effect_builder
                .put_signatures_to_storage(block_signatures)
                .events(move |_| should_upgrade.then(|| Event::Upgrade).into_iter()),
            Outcome::StoreBlock(block, execution_results) => async move {
                let block_hash = *block.hash();
                effect_builder.put_block_to_storage(block.clone()).await;
                effect_builder
                    .put_execution_results_to_storage(block_hash, execution_results)
                    .await;
                block
            }
            .event(|block| Event::PutBlockResult { block }),
            Outcome::Gossip(fs) => {
                let message = Message::FinalitySignature(fs);
                effect_builder.broadcast_message(message).ignore()
            }
            Outcome::AnnounceSignature(fs) => {
                effect_builder.announce_finality_signature(fs).ignore()
            }
            Outcome::AnnounceBlock(block) => effect_builder.announce_block_added(block).ignore(),
            Outcome::LoadSignatures(fs) => effect_builder
                .get_signatures_from_storage(fs.block_hash)
                .event(move |maybe_signatures| {
                    Event::GetStoredFinalitySignaturesResult(fs, maybe_signatures.map(Box::new))
                }),
            Outcome::VerifyIfBonded {
                new_fs,
                known_fs,
                protocol_version,
                latest_state_root_hash,
            } => effect_builder
                .is_bonded_validator(
                    new_fs.public_key.clone(),
                    new_fs.era_id,
                    latest_state_root_hash,
                    protocol_version,
                )
                .result(
                    |is_bonded| Event::IsBonded(known_fs, new_fs, is_bonded),
                    |error| {
                        error!(%error, "checking in future eras returned an error.");
                        panic!("couldn't check if validator is bonded")
                    },
                ),
        })
        .concat()
}

impl<REv> Component<REv> for LinearChainComponent
where
    REv: From<StorageRequest>
        + From<NetworkRequest<Message>>
        + From<LinearChainAnnouncement>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + Send,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::NewLinearChainBlock {
                block,
                execution_results,
            } => {
                let outcomes = self
                    .linear_chain_state
                    .handle_new_block(block, execution_results);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::PutBlockResult { block } => {
                let completion_duration = block.header().timestamp().elapsed().millis();
                self.metrics
                    .block_completion_duration
                    .set(completion_duration as i64);
                let outcomes = self.linear_chain_state.handle_put_block(block);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::FinalitySignatureReceived(fs, gossiped) => {
                let outcomes = self
                    .linear_chain_state
                    .handle_finality_signature(fs, gossiped);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::GetStoredFinalitySignaturesResult(fs, maybe_signatures) => {
                let outcomes = self
                    .linear_chain_state
                    .handle_cached_signatures(maybe_signatures, fs);
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::IsBonded(maybe_known_signatures, new_fs, is_bonded) => {
                let outcomes = self.linear_chain_state.handle_is_bonded(
                    maybe_known_signatures,
                    new_fs,
                    is_bonded,
                );
                outcomes_to_effects(effect_builder, outcomes)
            }
            Event::Upgrade => {
                self.stop_for_upgrade = true;
                Effects::new()
            }
            Event::GotUpgradeActivationPoint(activation_point) => {
                self.linear_chain_state
                    .got_upgrade_activation_point(activation_point);
                Effects::new()
            }
        }
    }
}

/// Returns the validator weights that should be used to check the finality signatures for the given
/// block.
pub(crate) async fn era_validator_weights_for_block<REv>(
    block_header: &BlockHeader,
    effect_builder: EffectBuilder<REv>,
) -> Result<(EraId, BTreeMap<PublicKey, U512>), Error>
where
    REv: From<StorageRequest> + From<ContractRuntimeRequest>,
{
    let era_for_validators_retrieval = block_header.era_id().saturating_sub(1);
    let switch_block_of_previous_era = effect_builder
        .get_switch_block_header_at_era_id_from_storage(era_for_validators_retrieval)
        .await
        .ok_or(Error::NoSwitchBlockForEra {
            era_id: era_for_validators_retrieval,
        })?;
    if block_header.protocol_version() != switch_block_of_previous_era.protocol_version() {
        if let Some(next_validator_weights) = block_header.next_era_validator_weights() {
            let request = EraValidatorsRequest::new(
                *switch_block_of_previous_era.state_root_hash(),
                switch_block_of_previous_era.protocol_version(),
            );
            let validator_map = effect_builder
                .get_era_validators_from_contract_runtime(request)
                .await
                .map_err(Error::GetEraValidators)?;
            let next_era_id = block_header.next_block_era_id();
            if let Some(next_validator_weights_according_to_previous_block) =
                validator_map.get(&next_era_id)
            {
                if next_validator_weights_according_to_previous_block != next_validator_weights {
                    // The validator weights that had been assigned to next_era_id before the
                    // upgrade don't match the ones in block_header. So the validators were changed
                    // as part of the upgrade. That usually means that the ones before the upgrade
                    // cannot be trusted anymore, and we expect the new validators to sign this
                    // block.
                    info!(
                        ?switch_block_of_previous_era,
                        ?next_validator_weights_according_to_previous_block,
                        first_block_after_the_upgrade = ?block_header,
                        next_validator_weights_after_the_upgrade = ?next_validator_weights,
                        "validator map changed in upgrade"
                    );
                    return Ok((block_header.era_id(), next_validator_weights.clone()));
                }
            } else {
                return Err(Error::MissingValidatorMapEntry {
                    block_header: Box::new(switch_block_of_previous_era),
                    missing_era_id: block_header.next_block_era_id(),
                });
            }
        }
    }
    let validator_weights = switch_block_of_previous_era
        .next_era_validator_weights()
        .ok_or(Error::MissingNextEraValidators {
            height: switch_block_of_previous_era.height(),
            era_id: era_for_validators_retrieval,
        })?;
    Ok((era_for_validators_retrieval, validator_weights.clone()))
}
