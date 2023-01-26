use crate::{
    components::gossiper::{Event, GossipItem, Gossiper, ItemProvider},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{Block, BlockHash, NodeId},
};

impl ItemProvider<Block> for Gossiper<{ Block::ID_IS_COMPLETE_ITEM }, Block> {
    fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: BlockHash,
        requester: NodeId,
    ) -> Effects<Event<Block>> {
        effect_builder
            .get_block_from_storage(item_id)
            .event(move |results| {
                let result = results.ok_or_else(|| String::from("block not found"));
                Event::GetFromStorageResult {
                    item_id,
                    requester,
                    result: Box::new(result),
                }
            })
    }
}
