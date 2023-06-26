use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;
use tracing::error;

use casper_types::{ApprovalsHash, Deploy, DeployConfigurationFailure, DeployId, Digest};

use crate::{
    components::fetcher::{
        metrics::Metrics, EmptyValidationMetadata, FetchItem, Fetcher, ItemFetcher, ItemHandle,
        StoringState, Tag,
    },
    effect::{requests::StorageRequest, EffectBuilder},
    types::{FinalizedApprovals, NodeId},
};

impl FetchItem for Deploy {
    type Id = DeployId;
    type ValidationError = DeployConfigurationFailure;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::Deploy;

    fn fetch_id(&self) -> Self::Id {
        let deploy_hash = *self.hash();
        let approvals_hash = self.compute_approvals_hash().unwrap_or_else(|error| {
            error!(%error, "failed to serialize approvals");
            ApprovalsHash::from(Digest::default())
        });
        DeployId::new(deploy_hash, approvals_hash)
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.is_valid()
    }
}

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

    async fn get_locally<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: DeployId,
    ) -> Option<Deploy> {
        effect_builder.get_stored_deploy(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: Deploy,
    ) -> StoringState<'a, Deploy> {
        StoringState::Enqueued(
            async move {
                let is_new = effect_builder
                    .put_deploy_to_storage(Arc::new(item.clone()))
                    .await;
                // If `is_new` is `false`, the deploy was previously stored, and the incoming
                // deploy could have a different set of approvals to the one already stored.
                // We can treat the incoming approvals as finalized and now try and store them.
                if !is_new {
                    effect_builder
                        .store_finalized_approvals(
                            *item.hash(),
                            FinalizedApprovals::new(item.approvals().clone()),
                        )
                        .await;
                }
            }
            .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: Deploy,
        _peer: NodeId,
    ) {
    }
}
