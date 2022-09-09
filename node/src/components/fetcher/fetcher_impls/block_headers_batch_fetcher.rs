use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockHeadersBatch, BlockHeadersBatchId, NodeId},
};

impl ItemFetcher<BlockHeadersBatch> for Fetcher<BlockHeadersBatch> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHeadersBatchId, HashMap<NodeId, Vec<FetchResponder<BlockHeadersBatch>>>>
    {
        &mut self.responders
    }

    fn validation_metadata(&self) -> &() {
        &()
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
        id: BlockHeadersBatchId,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<BlockHeadersBatch>,
    ) -> Effects<Event<BlockHeadersBatch>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_block_header_batch_from_storage(id)
            .event(move |maybe_batch| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(maybe_batch),
                responder,
            })
    }
}
