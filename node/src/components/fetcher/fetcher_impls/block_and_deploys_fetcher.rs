use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockAndDeploys, BlockHash, NodeId},
};

impl ItemFetcher<BlockAndDeploys> for Fetcher<BlockAndDeploys> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, Vec<FetchResponder<BlockAndDeploys>>>> {
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

    /// Gets a `BlockAndDeploys` from the storage component.
    fn get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: BlockHash,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<BlockAndDeploys>,
    ) -> Effects<Event<BlockAndDeploys>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_block_and_deploys_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(result),
                responder,
            })
    }

    fn put_to_storage<REv>(
        &self,
        item: BlockAndDeploys,
        peer: NodeId,
        effect_builder: EffectBuilder<REv>,
    ) -> Option<Effects<Event<BlockAndDeploys>>>
    where
        REv: From<StorageRequest> + Send,
    {
        let item = Box::new(item);
        Some(
            effect_builder
                .put_block_and_deploys_to_storage(item.clone())
                .event(move |_| Event::PutToStorage { item, peer }),
        )
    }
}
