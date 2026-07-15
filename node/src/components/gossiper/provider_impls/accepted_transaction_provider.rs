use async_trait::async_trait;

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{GossipedTransaction, GossipedTransactionId},
};

impl GossipItem for GossipedTransaction {
    type Id = GossipedTransactionId;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    fn gossip_id(&self) -> Self::Id {
        self.accepted_id()
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl LargeGossipItem for GossipedTransaction {}

#[async_trait]
impl ItemProvider<GossipedTransaction>
    for Gossiper<{ GossipedTransaction::ID_IS_COMPLETE_ITEM }, GossipedTransaction>
{
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: GossipedTransactionId,
    ) -> bool {
        let block_hash = item_id.block_hash();
        let transaction_id = item_id.transaction_id();

        effect_builder.is_transaction_stored(transaction_id).await
            && effect_builder.is_block_stored(block_hash).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: GossipedTransactionId,
    ) -> Option<Box<GossipedTransaction>> {
        let block_id = item_id.block_hash();

        if !effect_builder.is_block_stored(block_id).await {
            return None;
        }

        let transaction_id = item_id.transaction_id();

        effect_builder
            .get_stored_transaction(transaction_id)
            .await
            .map(|txn| Box::new(GossipedTransaction::new(txn, block_id)))
    }
}
