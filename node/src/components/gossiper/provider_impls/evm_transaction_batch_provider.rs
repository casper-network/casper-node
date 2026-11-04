use async_trait::async_trait;

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
    types::EvmTransactionBatchId,
};

/// A batch is gossiped as its transaction IDs only; the transactions themselves are fetched one at
/// a time by the EVM transaction batcher.
///
/// The ID is gossiped as a large item even though it is complete in itself, so that a node passes
/// a batch on only once it has fetched and accepted all of the batch's transactions: the gossiper
/// announces the received ID as a new item body, and starts gossiping it onwards only when the
/// batcher reports the batch as accepted.
impl GossipItem for EvmTransactionBatchId {
    type Id = EvmTransactionBatchId;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    fn gossip_id(&self) -> Self::Id {
        self.clone()
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl LargeGossipItem for EvmTransactionBatchId {}

/// A batch is not stored as such; it is held when all of its transactions are stored.
#[async_trait]
impl ItemProvider<EvmTransactionBatchId>
    for Gossiper<{ EvmTransactionBatchId::ID_IS_COMPLETE_ITEM }, EvmTransactionBatchId>
{
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: EvmTransactionBatchId,
    ) -> bool {
        for transaction_id in item_id.transaction_ids() {
            if !effect_builder.is_transaction_stored(*transaction_id).await {
                return false;
            }
        }
        true
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: EvmTransactionBatchId,
    ) -> Option<Box<EvmTransactionBatchId>> {
        // Only offer the batch to a peer if we can serve the transactions it will then fetch.
        Self::is_stored(effect_builder, item_id.clone())
            .await
            .then(|| Box::new(item_id))
    }
}
