use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockHeadersBatch, BlockHeadersBatchId, NodeId},
};

#[async_trait]
impl ItemFetcher<BlockHeadersBatch> for Fetcher<BlockHeadersBatch> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHeadersBatchId, HashMap<NodeId, ItemHandle<BlockHeadersBatch>>> {
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
        id: BlockHeadersBatchId,
    ) -> Option<BlockHeadersBatch> {
        effect_builder.get_block_header_batch_from_storage(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: BlockHeadersBatch,
    ) -> StoringState<'a, BlockHeadersBatch> {
        StoringState::Enqueued(
            effect_builder
                .put_block_headers_batch_to_storage(item.into_inner())
                .map(|_| ())
                .boxed(),
        )
    }
}
