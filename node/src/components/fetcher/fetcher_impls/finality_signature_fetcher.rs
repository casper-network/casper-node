use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use futures::FutureExt;

use casper_types::{crypto, FinalitySignature, FinalitySignatureId};

use crate::{
    components::fetcher::{
        metrics::Metrics, EmptyValidationMetadata, FetchItem, Fetcher, ItemFetcher, ItemHandle,
        StoringState, Tag,
    },
    effect::{
        announcements::FetchedNewFinalitySignatureAnnouncement,
        requests::{BlockAccumulatorRequest, StorageRequest},
        EffectBuilder,
    },
    types::NodeId,
};

impl FetchItem for FinalitySignature {
    type Id = Box<FinalitySignatureId>;
    type ValidationError = crypto::Error;
    type ValidationMetadata = EmptyValidationMetadata;

    const TAG: Tag = Tag::FinalitySignature;

    fn fetch_id(&self) -> Self::Id {
        Box::new(FinalitySignatureId::new(
            *self.block_hash(),
            self.era_id(),
            self.public_key().clone(),
        ))
    }

    fn validate(&self, _metadata: &EmptyValidationMetadata) -> Result<(), Self::ValidationError> {
        self.is_verified()
    }
}

#[async_trait]
impl ItemFetcher<FinalitySignature> for Fetcher<FinalitySignature> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<Box<FinalitySignatureId>, HashMap<NodeId, ItemHandle<FinalitySignature>>>
    {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_locally<REv: From<StorageRequest> + From<BlockAccumulatorRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        id: Box<FinalitySignatureId>,
    ) -> Option<FinalitySignature> {
        effect_builder
            .get_signature_from_storage(*id.block_hash(), id.public_key().clone())
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

    async fn announce_fetched_new_item<REv>(
        effect_builder: EffectBuilder<REv>,
        item: FinalitySignature,
        peer: NodeId,
    ) where
        REv: From<FetchedNewFinalitySignatureAnnouncement> + Send,
    {
        effect_builder
            .announce_fetched_new_finality_signature(Box::new(item.clone()), peer)
            .await
    }
}
