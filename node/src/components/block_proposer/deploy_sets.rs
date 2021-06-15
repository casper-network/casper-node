use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;

use super::{event::DeployInfo, BlockHeight, FinalizationQueue};
use crate::types::{DeployHash, DeployHeader, Timestamp};

/// Stores the internal state of the BlockProposer.
#[derive(Clone, DataSize, Debug, Default)]
pub(super) struct BlockProposerDeploySets {
    /// The collection of deploys pending for inclusion in a block, each added when the gossiper
    /// announces it has finished gossiping it.
    pub(super) pending_deploys: HashMap<DeployHash, DeployInfo>,
    /// The collection of transfers pending for inclusion in a block, each added when the gossiper
    /// announces it has finished gossiping it.
    pub(super) pending_transfers: HashMap<DeployHash, DeployInfo>,
    /// The deploys that have already been included in a finalized block.
    pub(super) finalized_deploys: HashMap<DeployHash, DeployHeader>,
    /// The next block height we expect to be finalized.
    /// If we receive a notification of finalization of a later block, we will store it in
    /// finalization_queue.
    /// If we receive a request that contains a later next_finalized, we will store it in
    /// request_queue.
    pub(super) next_finalized: BlockHeight,
    /// The queue of finalized block contents awaiting inclusion in `self.finalized_deploys`.
    pub(super) finalization_queue: FinalizationQueue,
}

impl BlockProposerDeploySets {
    /// Constructs the instance of `BlockProposerDeploySets` from the list of finalized deploys.
    pub(super) fn from_finalized(
        finalized_deploys: Vec<(DeployHash, DeployHeader)>,
        next_finalized_height: u64,
    ) -> BlockProposerDeploySets {
        BlockProposerDeploySets {
            finalized_deploys: finalized_deploys.into_iter().collect(),
            next_finalized: next_finalized_height,
            ..Default::default()
        }
    }
}

impl Display for BlockProposerDeploySets {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "(pending:{}, finalized:{})",
            self.pending_deploys.len() + self.pending_transfers.len(),
            self.finalized_deploys.len()
        )
    }
}

impl BlockProposerDeploySets {
    /// Prunes expired deploy information from the BlockProposerState, returns the total deploys
    /// pruned
    pub(crate) fn prune(&mut self, current_instant: Timestamp) -> usize {
        let pending_deploys = prune_pending_deploys(&mut self.pending_deploys, current_instant);
        let pending_transfers = prune_pending_deploys(&mut self.pending_transfers, current_instant);
        let finalized = prune_deploys(&mut self.finalized_deploys, current_instant);
        pending_deploys + pending_transfers + finalized
    }
}

/// Prunes expired deploy information from an individual deploy collection, returns the total
/// deploys pruned
pub(super) fn prune_deploys(
    deploys: &mut HashMap<DeployHash, DeployHeader>,
    current_instant: Timestamp,
) -> usize {
    let initial_len = deploys.len();
    deploys.retain(|_hash, header| !header.expired(current_instant));
    initial_len - deploys.len()
}

/// Prunes expired deploy information from an individual pending deploy collection, returns the
/// total deploys pruned
pub(super) fn prune_pending_deploys(
    deploys: &mut HashMap<DeployHash, DeployInfo>,
    current_instant: Timestamp,
) -> usize {
    let initial_len = deploys.len();
    deploys.retain(|_hash, deploy_info| !deploy_info.header.expired(current_instant));
    initial_len - deploys.len()
}
