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
    types::{BlockHash, NodeId, VersionedBlock},
};

#[async_trait]
impl ItemFetcher<VersionedBlock> for Fetcher<VersionedBlock> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<VersionedBlock>>> {
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
    ) -> Option<VersionedBlock> {
        effect_builder.get_versioned_block_from_storage(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: VersionedBlock,
    ) -> StoringState<'a, VersionedBlock> {
        StoringState::Enqueued(
            effect_builder
                .put_versioned_block_to_storage(Arc::new(item))
                .map(|_| ())
                .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: From<FetchedNewBlockAnnouncement> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: VersionedBlock,
        peer: NodeId,
    ) {
        // We fetch and store `VersionedBlock`, but announce as a most recent version
        effect_builder
            // TODO[RC]: Ideally, we'd like to yield `Block` instead of `VersionedBlock`, so that
            // the `VersionedBlock` stays internal to the fetcher. This requires some
            // trait juggling, so I'll leave it for later, since it's not strictly
            // needed for the versioning to operate. i.e.:
            //.announce_fetched_new_block(Arc::new(item.into()), peer)
            .announce_fetched_new_block(Arc::new(item), peer)
            .await
    }
}
