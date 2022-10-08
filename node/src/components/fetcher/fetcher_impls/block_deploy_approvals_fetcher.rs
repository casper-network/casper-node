use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;

use crate::{
    components::fetcher::{metrics::Metrics, Fetcher, ItemFetcher, ItemHandle},
    effect::{requests::StorageRequest, EffectBuilder},
    types::{BlockDeployApprovals, BlockHash, FinalizedApprovals, NodeId},
};

#[async_trait]
impl ItemFetcher<BlockDeployApprovals> for Fetcher<BlockDeployApprovals> {
    const SAFE_TO_RESPOND_TO_ALL: bool = true;

    fn item_handles(
        &mut self,
    ) -> &mut HashMap<BlockHash, HashMap<NodeId, ItemHandle<BlockDeployApprovals>>> {
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
        id: BlockHash,
    ) -> Option<BlockDeployApprovals> {
        let maybe_block_and_deploys = effect_builder
            .get_block_and_finalized_deploys_from_storage(id)
            .await;
        maybe_block_and_deploys.map(|block_and_deploys| {
            let approvals = block_and_deploys.deploys.into_iter().map(|deploy| {
                let deploy_approvals = deploy.approvals().clone();
                (*deploy.hash(), FinalizedApprovals::new(deploy_approvals))
            });
            BlockDeployApprovals::new(id, approvals.collect())
        })
    }
}
