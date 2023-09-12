use async_trait::async_trait;
use std::convert::TryInto;

use casper_types::{BlockHash, BlockV2};

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
};

impl GossipItem for BlockV2 {
    type Id = BlockHash;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = true;

    fn gossip_id(&self) -> Self::Id {
        *self.hash()
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::Mixed(self.era_id())
    }
}

impl LargeGossipItem for BlockV2 {}

#[async_trait]
impl ItemProvider<BlockV2> for Gossiper<{ BlockV2::ID_IS_COMPLETE_ITEM }, BlockV2> {
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: BlockHash,
    ) -> bool {
        effect_builder.is_block_stored(item_id).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: BlockHash,
    ) -> Option<Box<BlockV2>> {
        // TODO: Make `get_block_from_storage` return a boxed block instead of boxing here.
        if let Some(block) = effect_builder.get_block_from_storage(item_id).await {
            block.try_into().ok().map(Box::new)
        } else {
            None
        }
    }
}
