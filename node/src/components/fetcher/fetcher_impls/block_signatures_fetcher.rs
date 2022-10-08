use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle, StoringState},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockHash, BlockSignatures, NodeId},
};

#[async_trait]
impl ItemFetcher<BlockSignatures> for Fetcher<BlockSignatures> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<BlockSignatures>>> {
        &mut self.item_handles
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        _effect_builder: EffectBuilder<REv>,
        _id: BlockHash,
    ) -> Option<BlockSignatures> {
        todo!()
        // let fault_tolerance_fraction = self.fault_tolerance_fraction;
        // let block_header_with_metadata = effect_builder
        //     .get_block_header_with_metadata_from_storage(id, false)
        //     .await?;
        // has_enough_block_signatures(
        //     effect_builder,
        //     &block_header_with_metadata.block_header,
        //     &block_header_with_metadata.block_signatures,
        //     fault_tolerance_fraction,
        // )
        // .await
        // .then_some(block_header_with_metadata.block_signatures)
    }

    fn put_to_storage<'a, REv: From<StorageRequest> + Send>(
        _effect_builder: EffectBuilder<REv>,
        _item: BlockSignatures,
    ) -> StoringState<'a, BlockSignatures> {
        todo!()
    }
}
