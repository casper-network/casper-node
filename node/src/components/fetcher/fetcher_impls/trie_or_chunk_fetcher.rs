use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use tracing::error;

use casper_storage::data_access_layer::{TrieElement, TrieRequest, TrieResult};

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
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

    async fn get_locally<REv: From<ContractRuntimeRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: TrieOrChunkId,
    ) -> Option<TrieOrChunk> {
        let TrieOrChunkId(chunk_index, trie_key) = id;
        let request = TrieRequest::new(trie_key, Some(chunk_index));
        let result = effect_builder.get_trie(request).await;
        match result {
            TrieResult::ValueNotFound(_) => None,
            TrieResult::Failure(err) => {
                error!(%err, "failed to get trie element locally");
                None
            }
            TrieResult::Success { element } => match element {
                TrieElement::Raw(raw) => match TrieOrChunk::new(raw.into(), 0) {
                    Ok(voc) => Some(voc),
                    Err(err) => {
                        error!(%err, "raw chunking error");
                        None
                    }
                },
                TrieElement::Chunked(raw, chunk_id) => match TrieOrChunk::new(raw.into(), chunk_id)
                {
                    Ok(voc) => Some(voc),
                    Err(err) => {
                        error!(%err, "chunking error");
                        None
                    }
                },
            },
        }
    }

    fn put_to_storage<'a, REv>(
        _effect_builder: EffectBuilder<REv>,
        item: TrieOrChunk,
    ) -> StoringState<'a, TrieOrChunk> {
        // Stored by the GlobalStateSynchronizer once all chunks are fetched.
        StoringState::WontStore(item)
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: TrieOrChunk,
        _peer: NodeId,
    ) {
    }
}
