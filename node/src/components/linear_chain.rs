mod event;
mod metrics;
mod pending_signatures;
mod signature;
mod signature_cache;
mod state;

use std::convert::Infallible;

use datasize::DataSize;
use itertools::Itertools;
use num::rational::Ratio;
use prometheus::Registry;
use tracing::error;

use casper_types::{EraId, ProtocolVersion};

use self::{
    metrics::Metrics,
    state::{LinearChain, Outcome, Outcomes},
};
use crate::{
    components::Component,
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, EffectResultExt, Effects,
    },
    protocol::Message,
    types::ActivationPoint,
    NodeRng,
};
pub(crate) use event::Event;

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainComponent {
    linear_chain_state: LinearChain,
    #[data_size(skip)]
    metrics: Metrics,
    /// If true, the process should stop execution to allow an upgrade to proceed.
    stop_for_upgrade: bool,
    verifiable_chunked_hash_activation: EraId,
}

impl LinearChainComponent {
    pub(crate) fn new(
        registry: &Registry,
        protocol_version: ProtocolVersion,
        auction_delay: u64,
        unbonding_delay: u64,
        finality_threshold_fraction: Ratio<u64>,
        next_upgrade_activation_point: Option<ActivationPoint>,
        verifiable_chunked_hash_activation: EraId,
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
            verifiable_chunked_hash_activation,
        })
    }

    pub(crate) fn stop_for_upgrade(&self) -> bool {
        self.stop_for_upgrade
    }

    fn verifiable_chunked_hash_activation(&self) -> EraId {
        self.verifiable_chunked_hash_activation
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
                let outcomes = self
                    .linear_chain_state
                    .handle_put_block(block, self.verifiable_chunked_hash_activation());
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
