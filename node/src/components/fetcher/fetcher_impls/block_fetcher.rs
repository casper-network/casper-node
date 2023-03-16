use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{
        announcements::FetchedNewBlockAnnouncement,
        requests::{BlockAccumulatorRequest, StorageRequest},
        EffectBuilder,
    },
    types::{Block, BlockHash, NodeId},
};

#[async_trait]
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

    async fn get_locally<REv: From<StorageRequest> + From<BlockAccumulatorRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: BlockHash,
    ) -> Option<Block> {
        effect_builder.get_block_from_storage(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: Block,
    ) -> StoringState<'a, Block> {
        StoringState::Enqueued(
            effect_builder
                .put_block_to_storage(Arc::new(item))
                .map(|_| ())
                .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: From<FetchedNewBlockAnnouncement> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: Block,
        peer: NodeId,
    ) {
        effect_builder
            .announce_fetched_new_block(Arc::new(item), peer)
            .await
    }
}
