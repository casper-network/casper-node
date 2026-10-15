use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;

use casper_types::{InvalidTransaction, TransactionId};

use crate::{
    components::fetcher::{
        metrics::Metrics, EmptyValidationMetadata, FetchItem, Fetcher, ItemFetcher, ItemHandle,
        StoringState, Tag,
    },
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BatchedTransaction, NodeId},
};

impl FetchItem for BatchedTransaction {
    type Id = TransactionId;
    type ValidationError = InvalidTransaction;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::BatchedTransaction;

    fn fetch_id(&self) -> Self::Id {
        self.transaction().compute_id()
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.transaction().verify()
    }
}

#[async_trait]
impl ItemFetcher<BatchedTransaction> for Fetcher<BatchedTransaction> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<TransactionId, HashMap<NodeId, ItemHandle<BatchedTransaction>>> {
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
    ) -> Option<BatchedTransaction> {
        effect_builder
            .get_stored_transaction(id)
            .await
            .map(BatchedTransaction::new)
    }

    /// Not stored here: the EVM transaction batcher hands the batch's transactions to the
    /// transaction acceptor once it holds all of them, and the acceptor stores them.
    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        _effect_builder: EffectBuilder<REv>,
        item: BatchedTransaction,
    ) -> StoringState<'a, BatchedTransaction> {
        StoringState::WontStore(item)
    }

    async fn announce_fetched_new_item<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: BatchedTransaction,
        _peer: NodeId,
    ) {
    }
}
