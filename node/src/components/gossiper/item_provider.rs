use async_trait::async_trait;

use super::GossipItem;
use crate::effect::{requests::StorageRequest, EffectBuilder};

#[async_trait]
pub(super) trait ItemProvider<T: GossipItem> {
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
    ) -> bool;

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: T::Id,
    ) -> Option<Box<T>>;
}
