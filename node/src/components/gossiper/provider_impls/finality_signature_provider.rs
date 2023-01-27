use crate::{
    components::gossiper::{Event, GossipItem, Gossiper, ItemProvider},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{FinalitySignature, FinalitySignatureId, NodeId},
};

impl ItemProvider<FinalitySignature>
    for Gossiper<{ FinalitySignature::ID_IS_COMPLETE_ITEM }, FinalitySignature>
{
    fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: FinalitySignatureId,
        requester: NodeId,
    ) -> Effects<Event<FinalitySignature>> {
        effect_builder
            .get_finality_signature_from_storage(item_id.clone())
            .event(move |results| {
                let result = results.ok_or_else(|| String::from("finality signature not found"));
                Event::GetFromStorageResult {
                    item_id,
                    requester,
                    result: Box::new(result),
                }
            })
    }
}
