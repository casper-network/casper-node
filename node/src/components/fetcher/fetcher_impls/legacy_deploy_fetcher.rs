use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{DeployHash, LegacyDeploy, NodeId},
};

impl ItemFetcher<LegacyDeploy> for Fetcher<LegacyDeploy> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<DeployHash, HashMap<NodeId, Vec<FetchResponder<LegacyDeploy>>>> {
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

    /// Gets a `LegacyDeploy` from the storage component.
    fn get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: DeployHash,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<LegacyDeploy>,
    ) -> Effects<Event<LegacyDeploy>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_stored_legacy_deploy(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(result),
                responder,
            })
    }

    fn put_to_storage<REv>(
        &self,
        _item: LegacyDeploy,
        _peer: NodeId,
        _effect_builder: EffectBuilder<REv>,
    ) -> Option<Effects<Event<LegacyDeploy>>>
    where
        REv: From<StorageRequest> + Send,
    {
        // Incoming deploys are routed to the deploy acceptor for validation before being stored.
        None
    }
}
