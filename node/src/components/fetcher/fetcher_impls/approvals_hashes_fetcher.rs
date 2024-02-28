use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use casper_storage::block_store::types::{ApprovalsHashes, ApprovalsHashesValidationError};
use futures::FutureExt;

use casper_types::{Block, BlockHash};

use crate::{
    components::fetcher::{
        metrics::Metrics, FetchItem, Fetcher, ItemFetcher, ItemHandle, StoringState, Tag,
    },
    effect::{requests::StorageRequest, EffectBuilder},
    types::NodeId,
};

impl FetchItem for ApprovalsHashes {
    type Id = BlockHash;
    type ValidationError = ApprovalsHashesValidationError;
    type ValidationMetadata = Block;

    const TAG: Tag = Tag::ApprovalsHashes;

    fn fetch_id(&self) -> Self::Id {
        *self.block_hash()
    }

    fn validate(&self, block: &Block) -> Result<(), Self::ValidationError> {
        self.verify(block)
    }
}

#[async_trait]
impl ItemFetcher<ApprovalsHashes> for Fetcher<ApprovalsHashes> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<ApprovalsHashes>>> {
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
    ) -> Option<ApprovalsHashes> {
        effect_builder.get_approvals_hashes_from_storage(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: ApprovalsHashes,
    ) -> StoringState<'a, ApprovalsHashes> {
        StoringState::Enqueued(
            effect_builder
                .put_approvals_hashes_to_storage(Box::new(item))
                .map(|_| ())
                .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: ApprovalsHashes,
        _peer: NodeId,
    ) {
    }
}
