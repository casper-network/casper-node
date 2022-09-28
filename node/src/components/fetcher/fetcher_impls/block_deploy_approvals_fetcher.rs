use std::{collections::HashMap, time::Duration};

use crate::{
    components::fetcher::{metrics::Metrics, Event, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockDeployApprovals, BlockHash, FinalizedApprovals, NodeId},
};

impl ItemFetcher<BlockDeployApprovals> for Fetcher<BlockDeployApprovals> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn responders(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, Vec<FetchResponder<BlockDeployApprovals>>>> {
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
        id: BlockHash,
        peer: NodeId,
        _validation_metadata: (),
        responder: FetchResponder<BlockDeployApprovals>,
    ) -> Effects<Event<BlockDeployApprovals>>
    where
        REv: From<StorageRequest> + Send,
    {
        effect_builder
            .get_block_and_finalized_deploys_from_storage(id)
            .event(move |result| Event::GetFromStorageResult {
                id,
                peer,
                validation_metadata: (),
                maybe_item: Box::new(result.map(|block_and_deploys| {
                    let approvals = block_and_deploys.deploys.into_iter().map(|deploy| {
                        let deploy_approvals = deploy.approvals().clone();
                        (*deploy.id(), FinalizedApprovals::new(deploy_approvals))
                    });
                    BlockDeployApprovals::new(id, approvals.collect())
                })),
                responder,
            })
    }
}
