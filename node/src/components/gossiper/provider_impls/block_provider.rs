use async_trait::async_trait;

use crate::{
    components::gossiper::{GossipItem, Gossiper, ItemProvider},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{Block, BlockHash},
};

#[async_trait]
impl ItemProvider<Block> for Gossiper<{ Block::ID_IS_COMPLETE_ITEM }, Block> {
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: BlockHash,
    ) -> bool {
        effect_builder.is_block_stored(item_id).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: BlockHash,
    ) -> Option<Box<Block>> {
        // TODO: Make `get_block_from_storage` return a boxed block instead of boxing here.
        effect_builder
            .get_block_from_storage(item_id)
            .await
            .map(Box::new)
    }
}
