use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use casper_types::{BlockHash, BlockValidationError, VersionedBlock};

use crate::{
    components::fetcher::{
        metrics::Metrics, EmptyValidationMetadata, FetchItem, Fetcher, ItemFetcher, ItemHandle,
        StoringState, Tag,
    },
    effect::{
        announcements::FetchedNewBlockAnnouncement,
        requests::{BlockAccumulatorRequest, StorageRequest},
        EffectBuilder,
    },
    types::NodeId,
};

impl FetchItem for VersionedBlock {
    type Id = BlockHash;
    type ValidationError = BlockValidationError;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::VersionedBlock;

    fn fetch_id(&self) -> Self::Id {
        *self.hash()
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.verify()
    }
}

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
        effect_builder
            .announce_fetched_new_block(Arc::new(item), peer)
            .await
    }
}
