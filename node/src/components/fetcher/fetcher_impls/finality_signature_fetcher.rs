use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{FinalitySignature, FinalitySignatureId, NodeId},
};

#[async_trait]
impl ItemFetcher<FinalitySignature> for Fetcher<FinalitySignature> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<FinalitySignatureId, HashMap<NodeId, ItemHandle<FinalitySignature>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: FinalitySignatureId,
    ) -> Option<FinalitySignature> {
        effect_builder
            .get_signature_from_storage(id.block_hash, id.public_key.clone())
            .await
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item: FinalitySignature,
    ) -> StoringState<'a, FinalitySignature> {
        StoringState::Enqueued(
            effect_builder
                .put_finality_signature_to_storage(item)
                .map(|_| ())
                .boxed(),
        )
    }
}
