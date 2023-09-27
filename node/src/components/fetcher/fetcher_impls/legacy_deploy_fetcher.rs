use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use casper_types::{Deploy, DeployHash, Transaction};

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{LegacyDeploy, NodeId},
};

#[async_trait]
impl ItemFetcher<LegacyDeploy> for Fetcher<LegacyDeploy> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<DeployHash, HashMap<NodeId, ItemHandle<LegacyDeploy>>> {
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
        id: DeployHash,
    ) -> Option<LegacyDeploy> {
        effect_builder.get_stored_legacy_deploy(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: LegacyDeploy,
    ) -> StoringState<'a, LegacyDeploy> {
        StoringState::Enqueued(
            effect_builder
                .put_transaction_to_storage(Transaction::from(Deploy::from(item)))
                .map(|_| ())
                .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: LegacyDeploy,
        _peer: NodeId,
    ) {
    }
}
