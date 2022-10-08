use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use tracing::error;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle},
    effect::{requests::ContractRuntimeRequest, EffectBuilder},
    types::{NodeId, TrieOrChunk, TrieOrChunkId},
};

#[async_trait]
impl ItemFetcher<TrieOrChunk> for Fetcher<TrieOrChunk> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<TrieOrChunkId, HashMap<NodeId, ItemHandle<TrieOrChunk>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_from_storage<REv: From<ContractRuntimeRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: TrieOrChunkId,
    ) -> Option<TrieOrChunk> {
        effect_builder.get_trie(id).await.unwrap_or_else(|error| {
            error!(?error, "get_trie_request");
            None
        })
    }
}
