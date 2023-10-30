use std::{collections::HashMap, convert::Infallible, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use casper_types::{BlockHash, BlockHeader};

use crate::{
    components::fetcher::{
        metrics::Metrics, EmptyValidationMetadata, FetchItem, Fetcher, ItemFetcher, ItemHandle,
        StoringState, Tag,
    },
    effect::{requests::StorageRequest, EffectBuilder},
    types::NodeId,
};

impl FetchItem for BlockHeader {
    type Id = BlockHash;
    type ValidationError = Infallible;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::BlockHeader;

    fn fetch_id(&self) -> Self::Id {
        self.block_hash()
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        // No need for further validation.  The received header has necessarily had its hash
        // computed to be the same value we used for the fetch ID if we got here.
        Ok(())
    }
}

#[async_trait]
impl ItemFetcher<BlockHeader> for Fetcher<BlockHeader> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<BlockHeader>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_locally<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: BlockHash,
    ) -> Option<BlockHeader> {
        // Requests from fetcher are not restricted by the block availability index.
        let only_from_available_block_range = false;
        effect_builder
            .get_block_header_from_storage(id, only_from_available_block_range)
            .await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: BlockHeader,
    ) -> StoringState<'a, BlockHeader> {
        StoringState::Enqueued(
            effect_builder
                .put_block_header_to_storage(Box::new(item))
                .map(|_| ())
                .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: BlockHeader,
        _peer: NodeId,
    ) {
    }
}
