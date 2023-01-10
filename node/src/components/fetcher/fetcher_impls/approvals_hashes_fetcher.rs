use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{ApprovalsHashes, BlockHash, NodeId},
};

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

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
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
}
