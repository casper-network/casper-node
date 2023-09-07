use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;
use tracing::error;

use casper_types::{
    Deploy, DeployApprovalsHash, DeployConfigurationFailure, DeployId, Digest, Transaction,
    TransactionId,
};

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
            DeployApprovalsHash::from(Digest::default())
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
        effect_builder
            .get_stored_transaction(TransactionId::from(id))
            .await
            .map(|txn| match txn {
                Transaction::Deploy(deploy) => deploy,
                Transaction::V1(_) => {
                    todo!(
                        "unreachable, but this code path will be removed as part of \
                        https://github.com/casper-network/roadmap/issues/189"
                    )
                }
            })
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: Deploy,
    ) -> StoringState<'a, Deploy> {
        StoringState::Enqueued(
            async move {
                let transaction = Transaction::from(item);
                let is_new = effect_builder
                    .put_transaction_to_storage(transaction.clone())
                    .await;
                // If `is_new` is `false`, the transaction was previously stored, and the incoming
                // transaction could have a different set of approvals to the one already stored.
                // We can treat the incoming approvals as finalized and now try and store them.
                if !is_new {
                    effect_builder
                        .store_finalized_approvals(
                            transaction.hash(),
                            FinalizedApprovals::new(&transaction),
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
