use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, Effects},
    types::{BlockHeaderWithMetadata, NodeId},
};

impl ItemFetcher<BlockHeaderWithMetadata> for Fetcher<BlockHeaderWithMetadata> {
    const SAFE_TO_RESPOND_TO_ALL: bool = false;

    fn responders(
        &mut self,
    ) -> &mut HashMap<u64, HashMap<NodeId, Vec<FetchResponder<BlockHeaderWithMetadata>>>> {
        &mut self.responders
    }

    fn validation_metadata(&self) -> &() {
        &()
    }

    fn metrics(&mut self) -> &Metrics {
        &self.metrics
    }

    fn peer_timeout(&self) -> Duration {
        self.get_from_peer_timeout
    }

    fn get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: u64,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<BlockHeaderWithMetadata>,
    ) -> Effects<Event<BlockHeaderWithMetadata>>
    where
        REv: From<StorageRequest> + Send,
    {
        todo!()
        // let fault_tolerance_fraction = self.fault_tolerance_fraction;
        // async move {
        //     let block_header_with_metadata = effect_builder
        //         .get_block_header_with_metadata_from_storage_by_height(id, false)
        //         .await?;
        //     has_enough_block_signatures(
        //         effect_builder,
        //         &block_header_with_metadata.block_header,
        //         &block_header_with_metadata.block_signatures,
        //         fault_tolerance_fraction,
        //     )
        //     .await
        //     .then_some(block_header_with_metadata)
        // }
        // .event(move |result| Event::GetFromStorageResult {
        //     id,
        //     peer,
        //     maybe_item: Box::new(result),
        //     responder,
        // })
    }
}
