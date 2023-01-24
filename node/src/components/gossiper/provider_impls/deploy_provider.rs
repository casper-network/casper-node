use crate::{
    components::gossiper::{Event, GossipItem, Gossiper, ItemProvider},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{Deploy, DeployId, NodeId},
};

impl ItemProvider<Deploy> for Gossiper<{ Deploy::ID_IS_COMPLETE_ITEM }, Deploy> {
    fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: DeployId,
        requester: NodeId,
    ) -> Effects<Event<Deploy>> {
        effect_builder
            .get_stored_deploy(item_id)
            .event(move |result| Event::GetFromStorageResult {
                item_id,
                requester,
                result: Box::new(
                    result.ok_or_else(|| format!("failed to get {} from storage", item_id)),
                ),
            })
    }
}
