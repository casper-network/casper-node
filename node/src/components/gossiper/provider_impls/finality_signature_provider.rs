use async_trait::async_trait;

use crate::{
    components::gossiper::{GossipItem, Gossiper, ItemProvider},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{FinalitySignature, FinalitySignatureId},
};

#[async_trait]
impl ItemProvider<FinalitySignature>
    for Gossiper<{ FinalitySignature::ID_IS_COMPLETE_ITEM }, FinalitySignature>
{
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: FinalitySignatureId,
    ) -> bool {
        effect_builder
            .is_finality_signature_stored(item_id.clone())
            .await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: FinalitySignatureId,
    ) -> Option<Box<FinalitySignature>> {
        // TODO: Make `get_finality_signature_from_storage` return a boxed copy instead.
        effect_builder
            .get_finality_signature_from_storage(item_id.clone())
            .await
            .map(Box::new)
    }
}
