mod event;
mod metrics;
mod pending_signatures;
mod signature;
mod signature_cache;
mod state;

use std::{convert::Infallible, fmt::Display};

use tracing::{debug, error, info, warn};

use super::Component;
use crate::{
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, LinearChainRequest, NetworkRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, EffectResultExt, Effects,
    },
    protocol::Message,
    types::{BlockByHeight, FinalitySignature, Timestamp},
    NodeRng,
};

pub use event::Event;
pub(crate) use state::LinearChain;

impl<I, REv> Component<REv> for LinearChain<I>
where
    REv: From<StorageRequest>
        + From<NetworkRequest<I, Message>>
        + From<LinearChainAnnouncement>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + Send,
    I: Display + Send + 'static,
{
    type Event = Event<I>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(LinearChainRequest::BlockRequest(block_hash, sender)) => async move {
                match effect_builder.get_block_from_storage(block_hash).await {
                    None => debug!("failed to get {} for {}", block_hash, sender),
                    Some(block) => match Message::new_get_response(&block) {
                        Ok(message) => effect_builder.send_message(sender, message).await,
                        Err(error) => error!("failed to create get-response {}", error),
                    },
                }
            }
            .ignore(),
            Event::Request(LinearChainRequest::BlockAtHeightLocal(height, responder)) => {
                async move {
                    let block = effect_builder
                        .get_block_at_height_from_storage(height)
                        .await;
                    responder.respond(block).await
                }
                .ignore()
            }
            Event::Request(LinearChainRequest::BlockAtHeight(height, sender)) => async move {
                let block_by_height = match effect_builder
                    .get_block_at_height_from_storage(height)
                    .await
                {
                    None => {
                        debug!("failed to get {} for {}", height, sender);
                        BlockByHeight::Absent(height)
                    }
                    Some(block) => BlockByHeight::new(block),
                };
                match Message::new_get_response(&block_by_height) {
                    Ok(message) => effect_builder.send_message(sender, message).await,
                    Err(error) => {
                        error!("failed to create get-response {}", error);
                    }
                }
            }
            .ignore(),
            Event::NewLinearChainBlock {
                block,
                execution_results,
            } => {
                let (signatures, mut effects) = self.collect_pending_finality_signatures(
                    block.hash(),
                    block.header().era_id(),
                    effect_builder,
                );
                // Cache the signature as we expect more finality signatures to arrive soon.
                self.cache_signatures(signatures.clone());
                effects.extend(
                    effect_builder
                        .put_signatures_to_storage(signatures)
                        .ignore(),
                );
                let block_to_store = block.clone();
                effects.extend(
                    async move {
                        let block_hash = *block_to_store.hash();
                        effect_builder.put_block_to_storage(block_to_store).await;
                        effect_builder
                            .put_execution_results_to_storage(block_hash, execution_results)
                            .await;
                    }
                    .event(move |_| Event::PutBlockResult { block }),
                );
                effects
            }
            Event::PutBlockResult { block } => {
                self.latest_block = Some(*block.clone());

                let completion_duration =
                    Timestamp::now().millis() - block.header().timestamp().millis();
                self.metrics
                    .block_completion_duration
                    .set(completion_duration as i64);

                let block_hash = block.header().hash();
                let era_id = block.header().era_id();
                let height = block.header().height();
                info!(%block_hash, %era_id, %height, "linear chain block stored");
                effect_builder.announce_block_added(block).ignore()
            }
            Event::FinalitySignatureReceived(fs, gossiped) => {
                let FinalitySignature {
                    block_hash, era_id, ..
                } = *fs;
                if !self.add_pending_finality_signature(*fs.clone(), gossiped) {
                    return Effects::new();
                }
                match self.signature_cache.get(&block_hash, era_id) {
                    None => effect_builder
                        .get_signatures_from_storage(block_hash)
                        .event(move |maybe_signatures| {
                            let maybe_box_signatures = maybe_signatures.map(Box::new);
                            Event::GetStoredFinalitySignaturesResult(fs, maybe_box_signatures)
                        }),
                    Some(signatures) => effect_builder.immediately().event(move |_| {
                        Event::GetStoredFinalitySignaturesResult(fs, Some(Box::new(signatures)))
                    }),
                }
            }
            Event::GetStoredFinalitySignaturesResult(fs, maybe_signatures) => {
                if let Some(signatures) = &maybe_signatures {
                    if signatures.era_id != fs.era_id {
                        warn!(public_key=%fs.public_key,
                            expected=%signatures.era_id, got=%fs.era_id,
                            "finality signature with invalid era id.");
                        // TODO: Disconnect from the sender.
                        self.remove_from_pending_fs(&*fs);
                        return Effects::new();
                    }
                    // Populate cache so that next finality signatures don't have to read from the
                    // storage. If signature is already from cache then this will be a noop.
                    self.cache_signatures(*signatures.clone());
                }
                // Check if the validator is bonded in the era in which the block was created.
                // TODO: Use protocol version that is valid for the block's height.
                let protocol_version = self.protocol_version;
                let latest_state_root_hash = self
                    .latest_block
                    .as_ref()
                    .map(|block| *block.header().state_root_hash());
                effect_builder
                    .is_bonded_validator(
                        fs.public_key,
                        fs.era_id,
                        latest_state_root_hash,
                        protocol_version,
                    )
                    .result(
                        |is_bonded| Event::IsBonded(maybe_signatures, fs, is_bonded),
                        |error| {
                            error!(%error, "checking in future eras returned an error.");
                            panic!("couldn't check if validator is bonded")
                        },
                    )
            }
            Event::IsBonded(Some(mut signatures), fs, true) => {
                // Known block and signature from a bonded validator.
                // Check if we had already seen this signature before.
                let signature_known = signatures
                    .proofs
                    .get(&fs.public_key)
                    .iter()
                    .any(|sig| *sig == &fs.signature);
                // If new, gossip and store.
                if signature_known {
                    self.remove_from_pending_fs(&*fs);
                    Effects::new()
                } else {
                    let mut effects = effect_builder
                        .announce_finality_signature(fs.clone())
                        .ignore();
                    if let Some(signature) = self.remove_from_pending_fs(&*fs) {
                        if signature.is_local() {
                            let message = Message::FinalitySignature(fs.clone());
                            effects.extend(effect_builder.broadcast_message(message).ignore());
                        }
                    };
                    signatures.insert_proof(fs.public_key, fs.signature);
                    // Cache the results in case we receive the same finality signature before we
                    // manage to store it in the database.
                    self.cache_signatures(*signatures.clone());
                    debug!(hash=%signatures.block_hash, "storing finality signatures");
                    effects.extend(
                        effect_builder
                            .put_signatures_to_storage(*signatures)
                            .ignore(),
                    );
                    effects
                }
            }
            Event::IsBonded(None, _fs, true) => {
                // Unknown block but validator is bonded.
                // We should finalize the same block eventually. Either in this or in the
                // next era.
                Effects::new()
            }
            Event::IsBonded(Some(_), fs, false) | Event::IsBonded(None, fs, false) => {
                self.remove_from_pending_fs(&fs);
                // Unknown validator.
                let FinalitySignature {
                    public_key,
                    block_hash,
                    ..
                } = *fs;
                warn!(
                    validator = %public_key,
                    %block_hash,
                    "Received a signature from a validator that is not bonded."
                );
                // TODO: Disconnect from the sender.
                Effects::new()
            }
        }
    }
}
