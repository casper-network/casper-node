use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{
        metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher, ItemHandle,
    },
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{FinalitySignature, FinalitySignatureId, NodeId},
};

impl ItemFetcher<FinalitySignature> for Fetcher<FinalitySignature> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<FinalitySignatureId, HashMap<NodeId, ItemHandle<FinalitySignature>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: FinalitySignatureId,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<FinalitySignature>,
    ) -> Effects<Event<FinalitySignature>>
    where
        REv: From<StorageRequest> + Send,
    {
        let block_hash = id.block_hash;
        let public_key = id.public_key.clone();
        effect_builder
            .get_signature_from_storage(block_hash, public_key)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(result),
                responder,
            })
    }
}
