use async_trait::async_trait;

use casper_types::{FinalitySignature, FinalitySignatureId};

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
};

impl GossipItem for FinalitySignature {
    type Id = Box<FinalitySignatureId>;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = true;

    fn gossip_id(&self) -> Self::Id {
        Box::new(FinalitySignatureId::new(
            *self.block_hash(),
            self.era_id(),
            self.public_key().clone(),
        ))
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::Mixed(self.era_id())
    }
}

impl LargeGossipItem for FinalitySignature {}

#[async_trait]
impl ItemProvider<FinalitySignature>
    for Gossiper<{ FinalitySignature::ID_IS_COMPLETE_ITEM }, FinalitySignature>
{
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: Box<FinalitySignatureId>,
    ) -> bool {
        effect_builder.is_finality_signature_stored(item_id).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: Box<FinalitySignatureId>,
    ) -> Option<Box<FinalitySignature>> {
        // TODO: Make `get_finality_signature_from_storage` return a boxed copy instead.
        effect_builder
            .get_finality_signature_from_storage(item_id)
            .await
            .map(Box::new)
    }
}
