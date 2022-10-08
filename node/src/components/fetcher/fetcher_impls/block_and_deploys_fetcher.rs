use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockAndDeploys, BlockHash, NodeId},
};

#[async_trait]
impl ItemFetcher<BlockAndDeploys> for Fetcher<BlockAndDeploys> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<BlockAndDeploys>>> {
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
    ) -> Option<BlockAndDeploys> {
        effect_builder.get_block_and_deploys_from_storage(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: BlockAndDeploys,
    ) -> StoringState<'a, BlockAndDeploys> {
        StoringState::Enqueued(
            effect_builder
                .put_block_and_deploys_to_storage(Box::new(item))
                .map(|_| ())
                .boxed(),
        )
    }
}
