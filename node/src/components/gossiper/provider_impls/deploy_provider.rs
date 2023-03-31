use async_trait::async_trait;

use crate::{
    components::gossiper::{GossipItem, Gossiper, ItemProvider},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{Deploy, DeployId},
};

#[async_trait]
impl ItemProvider<Deploy> for Gossiper<{ Deploy::ID_IS_COMPLETE_ITEM }, Deploy> {
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: DeployId,
    ) -> bool {
        effect_builder.is_deploy_stored(item_id).await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: DeployId,
    ) -> Option<Box<Deploy>> {
        // TODO: Make `get_stored_deploy` return a boxed value instead of boxing here.
        effect_builder
            .get_stored_deploy(item_id)
            .await
            .map(Box::new)
    }
}
