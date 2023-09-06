use async_trait::async_trait;
use tracing::error;

use casper_types::{Deploy, DeployApprovalsHash, DeployId, Digest, Transaction, TransactionId};

use crate::{
    components::gossiper::{GossipItem, GossipTarget, Gossiper, ItemProvider, LargeGossipItem},
    effect::{requests::StorageRequest, EffectBuilder},
};

impl GossipItem for Deploy {
    type Id = DeployId;

    const ID_IS_COMPLETE_ITEM: bool = false;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    fn gossip_id(&self) -> Self::Id {
        let deploy_hash = *self.hash();
        let approvals_hash = self.compute_approvals_hash().unwrap_or_else(|error| {
            error!(%error, "failed to serialize approvals");
            DeployApprovalsHash::from(Digest::default())
        });
        DeployId::new(deploy_hash, approvals_hash)
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl LargeGossipItem for Deploy {}

#[async_trait]
impl ItemProvider<Deploy> for Gossiper<{ Deploy::ID_IS_COMPLETE_ITEM }, Deploy> {
    async fn is_stored<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: DeployId,
    ) -> bool {
        effect_builder
            .is_transaction_stored(TransactionId::from(item_id))
            .await
    }

    async fn get_from_storage<REv: From<StorageRequest> + Send>(
        effect_builder: EffectBuilder<REv>,
        item_id: DeployId,
    ) -> Option<Box<Deploy>> {
        effect_builder
            .get_stored_transaction(TransactionId::from(item_id))
            .await
            .map(|txn| match txn {
                Transaction::Deploy(deploy) => Box::new(deploy),
                Transaction::V1(_) => {
                    todo!(
                        "unreachable, but this code path will be removed as part of \
                        https://github.com/casper-network/roadmap/issues/188"
                    )
                }
            })
    }
}
