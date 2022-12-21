use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockHash, BlockHeader, NodeId},
};

#[async_trait]
impl ItemFetcher<BlockHeader> for Fetcher<BlockHeader> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<BlockHeader>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: BlockHash,
    ) -> Option<BlockHeader> {
        // Requests from fetcher are not restricted by the block availability index.
        let only_from_available_block_range = false;
        effect_builder
            .get_block_header_from_storage(id, only_from_available_block_range)
            .await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: BlockHeader,
    ) -> StoringState<'a, BlockHeader> {
        StoringState::Enqueued(
            effect_builder
                .put_block_header_to_storage(Box::new(item))
                .map(|_| ())
                .boxed(),
        )
    }
}
