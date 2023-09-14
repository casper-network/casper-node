use async_trait::async_trait;

use casper_types::{Transaction, TransactionId};

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
};

impl GossipItem for Transaction {
    type Id = TransactionId;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    fn gossip_id(&self) -> Self::Id {
        self.compute_id()
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl LargeGossipItem for Transaction {}

#[async_trait]
impl ItemProvider<Transaction> for Gossiper<{ Transaction::ID_IS_COMPLETE_ITEM }, Transaction> {
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: TransactionId,
    ) -> bool {
        effect_builder.is_transaction_stored(item_id).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: TransactionId,
    ) -> Option<Box<Transaction>> {
        effect_builder
            .get_stored_transaction(item_id)
            .await
            .map(Box::new)
    }
}
