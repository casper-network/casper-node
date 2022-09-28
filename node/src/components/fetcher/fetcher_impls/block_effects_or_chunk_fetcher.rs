use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockEffectsOrChunk, BlockEffectsOrChunkId, NodeId},
};

impl ItemFetcher<BlockEffectsOrChunk> for Fetcher<BlockEffectsOrChunk> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<
        BlockEffectsOrChunkId,
        HashMap<NodeId, Vec<FetchResponder<BlockEffectsOrChunk>>>,
    > {
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
        id: BlockEffectsOrChunkId,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<BlockEffectsOrChunk>,
    ) -> Effects<Event<BlockEffectsOrChunk>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_block_effects_or_chunk_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(result),
                responder,
            })
    }
}
