use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{DeployHash, FinalizedApprovals, FinalizedApprovalsWithId, NodeId},
};

impl ItemFetcher<FinalizedApprovalsWithId> for Fetcher<FinalizedApprovalsWithId> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<DeployHash, HashMap<NodeId, Vec<FetchResponder<FinalizedApprovalsWithId>>>>
    {
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

    /// Gets the finalized approvals for a deploy from the storage component.
    fn get_from_storage<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: DeployHash,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<FinalizedApprovalsWithId>,
    ) -> Effects<Event<FinalizedApprovalsWithId>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_deploys_from_storage(vec![id])
            .event(move |mut results| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(results.pop().expect("can only contain one result").map(
                    |deploy| {
                        FinalizedApprovalsWithId::new(
                            id,
                            FinalizedApprovals::new(deploy.into_naive().approvals().clone()),
                        )
                    },
                )),
                responder,
            })
    }
}
