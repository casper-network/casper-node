use async_trait::async_trait;
use tracing::error;

use crate::{
    components::{
        gossiper::{GossipItem, Gossiper, ItemProvider},
        network::GossipedAddress,
    },
    effect::EffectBuilder,
};

#[async_trait]
impl ItemProvider<GossipedAddress>
    for Gossiper<{ GossipedAddress::ID_IS_COMPLETE_ITEM }, GossipedAddress>
{
    async fn is_stored<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        item_id: GossipedAddress,
    ) -> bool {
        error!(%item_id, "address gossiper should never try to check if item is stored");
        false
    }

    async fn get_from_storage<REv: Send>(
        _effect_builder: EffectBuilder<REv>,
        item_id: GossipedAddress,
    ) -> Option<Box<GossipedAddress>> {
        error!(%item_id, "address gossiper should never try to get from storage");
        None
    }
}
