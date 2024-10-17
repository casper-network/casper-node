use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use casper_types::{InvalidTransaction, Transaction, TransactionId};

use crate::{
    components::fetcher::{
        metrics::Metrics, EmptyValidationMetadata, FetchItem, Fetcher, ItemFetcher, ItemHandle,
        StoringState, Tag,
    },
    effect::{requests::StorageRequest, EffectBuilder},
    types::NodeId,
};

impl FetchItem for Transaction {
    type Id = TransactionId;
    type ValidationError = InvalidTransaction;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::Transaction;

    fn fetch_id(&self) -> Self::Id {
        self.compute_id()
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.verify()
    }
}

#[async_trait]
impl ItemFetcher<Transaction> for Fetcher<Transaction> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<TransactionId, HashMap<NodeId, ItemHandle<Transaction>>> {
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
        id: TransactionId,
    ) -> Option<Transaction> {
        effect_builder.get_stored_transaction(id).await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: Transaction,
    ) -> StoringState<'a, Transaction> {
        StoringState::Enqueued(
            async move {
                let is_new = effect_builder
                    .put_transaction_to_storage(item.clone())
                    .await;
                // If `is_new` is `false`, the transaction was previously stored, and the incoming
                // transaction could have a different set of approvals to the one already stored.
                // We can treat the incoming approvals as finalized and now try and store them.
                if !is_new {
                    effect_builder
                        .store_finalized_approvals(item.hash(), item.approvals())
                        .await;
                }
            }
            .boxed(),
        )
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: Transaction,
        _peer: NodeId,
    ) {
    }
}
