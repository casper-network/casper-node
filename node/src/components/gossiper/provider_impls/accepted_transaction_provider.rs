use async_trait::async_trait;

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{AcceptedTransaction, AcceptedTransactionId},
};

impl GossipItem for AcceptedTransaction {
    type Id = AcceptedTransactionId;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    fn gossip_id(&self) -> Self::Id {
        self.accepted_id()
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl LargeGossipItem for AcceptedTransaction {}

#[async_trait]
impl ItemProvider<AcceptedTransaction>
    for Gossiper<{ AcceptedTransaction::ID_IS_COMPLETE_ITEM }, AcceptedTransaction>
{
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: AcceptedTransactionId,
    ) -> bool {
        let block_hash = item_id.block_hash();
        let transaction_id = item_id.transaction_id();

        effect_builder.is_transaction_stored(transaction_id).await
            && effect_builder.is_block_stored(block_hash).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: AcceptedTransactionId,
    ) -> Option<Box<AcceptedTransaction>> {
        let block_id = item_id.block_hash();

        if !effect_builder.is_block_stored(block_id).await {
            return None;
        }

        let transaction_id = item_id.transaction_id();

        effect_builder
            .get_stored_transaction(transaction_id)
            .await
            .map(|txn| Box::new(AcceptedTransaction::new(txn, block_id)))
    }
}
