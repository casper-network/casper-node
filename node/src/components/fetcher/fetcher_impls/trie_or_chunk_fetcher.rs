use std::{collections::HashMap, time::Duration};

use tracing::error;

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::ContractRuntimeRequest, EffectBuilder, EffectExt, Effects},
    types::{NodeId, TrieOrChunk, TrieOrChunkId},
};

impl ItemFetcher<TrieOrChunk> for Fetcher<TrieOrChunk> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<TrieOrChunkId, HashMap<NodeId, Vec<FetchResponder<TrieOrChunk>>>> {
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
        id: TrieOrChunkId,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<TrieOrChunk>,
    ) -> Effects<Event<TrieOrChunk>>
    where
        REv: From<ContractRuntimeRequest> + Send,
    {
        async move {
            let maybe_trie = match effect_builder.get_trie(id).await {
                Ok(maybe_trie) => maybe_trie,
                Err(error) => {
                    error!(?error, "get_trie_request");
                    None
                }
            };
            Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(maybe_trie),
                responder,
            }
        }
        .event(std::convert::identity)
    }
}
