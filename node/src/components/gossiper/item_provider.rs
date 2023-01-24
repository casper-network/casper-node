use super::{Event, GossipItem};
use crate::{
    effect::{requests::StorageRequest, EffectBuilder, Effects},
    types::NodeId,
};

pub(super) trait ItemProvider<T: GossipItem> {
    fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
        requester: NodeId,
    ) -> Effects<Event<T>>;
}
