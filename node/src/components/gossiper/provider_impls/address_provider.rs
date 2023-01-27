use crate::{
    components::{
        gossiper::{Event, GossipItem, Gossiper, ItemProvider},
        network::GossipedAddress,
    },
    effect::{EffectBuilder, Effects},
    types::NodeId,
};

impl ItemProvider<GossipedAddress>
    for Gossiper<{ GossipedAddress::ID_IS_COMPLETE_ITEM }, GossipedAddress>
{
    fn get_from_storage<REv>(
        _effect_builder: EffectBuilder<REv>,
        item_id: GossipedAddress,
        _requester: NodeId,
    ) -> Effects<Event<GossipedAddress>> {
        panic!("address gossiper should never try to get {}", item_id)
    }
}
