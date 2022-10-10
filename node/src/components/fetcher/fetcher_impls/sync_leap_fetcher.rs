use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockHash, NodeId, SyncLeap},
};

#[async_trait]
impl ItemFetcher<SyncLeap> for Fetcher<SyncLeap> {
    // We want the fetcher to ask all the peers we give to it separately, and return their
    // responses separately, not just respond with the first SyncLeap it successfully gets from a
    // single peer.
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn item_handles(&mut self) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<SyncLeap>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_from_storage<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _id: BlockHash,
    ) -> Option<SyncLeap> {
        // We never get a SyncLeap we requested from our own storage.
        None
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: SyncLeap,
    ) -> StoringState<'a, SyncLeap> {
        StoringState::Enqueued(
            async move {
                for header in item.headers() {
                    effect_builder
                        .put_block_header_to_storage(Box::new(header.clone()))
                        .await;
                }
                for signed_header in item.signed_block_headers {
                    effect_builder
                        .put_signatures_to_storage(signed_header.block_signatures)
                        .await;
                }
            }
            .boxed(),
        )
    }
}
