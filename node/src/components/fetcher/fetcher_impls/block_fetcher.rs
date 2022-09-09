use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{
        metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher, ItemHandle,
    },
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{Block, BlockHash, NodeId},
};

impl ItemFetcher<Block> for Fetcher<Block> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn item_handles(&mut self) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<Block>>> {
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
        id: BlockHash,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<Block>,
    ) -> Effects<Event<Block>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_block_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(result),
                responder,
            })
    }
}
