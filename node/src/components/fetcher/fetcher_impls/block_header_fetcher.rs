use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockHash, BlockHeader, NodeId},
};

impl ItemFetcher<BlockHeader> for Fetcher<BlockHeader> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, Vec<FetchResponder<BlockHeader>>>> {
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
        id: BlockHash,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<BlockHeader>,
    ) -> Effects<Event<BlockHeader>>
    where
        REv: From<StorageRequest> + Send,
    {
        // Requests from fetcher are not restricted by the block availability index.
        let only_from_available_block_range = false;

        effect_builder
            .get_block_header_from_storage(id, only_from_available_block_range)
            .event(move |maybe_block_header| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(maybe_block_header),
                responder,
            })
    }
}
