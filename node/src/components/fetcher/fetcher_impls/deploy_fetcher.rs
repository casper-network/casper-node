use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{Deploy, DeployId, NodeId},
};

#[async_trait]
impl ItemFetcher<Deploy> for Fetcher<Deploy> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(&mut self) -> &mut HashMap<DeployId, HashMap<NodeId, ItemHandle<Deploy>>> {
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
        id: DeployId,
    ) -> Option<Deploy> {
        effect_builder.get_stored_deploy(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        _effect_builder: EffectBuilder<REv>,
        item: Deploy,
    ) -> StoringState<'a, Deploy> {
        // Incoming deploys are routed to the deploy acceptor for validation before being stored.
        StoringState::WontStore(item)
    }
}
