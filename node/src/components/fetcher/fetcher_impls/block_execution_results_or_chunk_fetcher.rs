use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, NodeId},
};

#[async_trait]
impl ItemFetcher<BlockExecutionResultsOrChunk> for Fetcher<BlockExecutionResultsOrChunk> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<
        BlockExecutionResultsOrChunkId,
        HashMap<NodeId, ItemHandle<BlockExecutionResultsOrChunk>>,
    > {
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
        id: BlockExecutionResultsOrChunkId,
    ) -> Option<BlockExecutionResultsOrChunk> {
        effect_builder
            .get_block_execution_results_or_chunk_from_storage(id)
            .await
    }

    fn put_to_storage<'a, REv>(
        _effect_builder: EffectBuilder<REv>,
        item: BlockExecutionResultsOrChunk,
    ) -> StoringState<'a, BlockExecutionResultsOrChunk> {
        // Stored by the BlockSynchronizer once all chunks are fetched.
        StoringState::WontStore(item)
    }
}
