use std::{
    collections::{BTreeMap, HashMap},
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::{event::DeployType, BlockHeight, FinalizationQueue};
use crate::{
    types::{DeployHash, DeployHeader, Timestamp},
    Chainspec,
};
use casper_execution_engine::shared::gas::Gas;

// Key for pending collection: willingness to pay (header.gas_price), header.payment_amount(),
// deploy_hash
type PendingKey = (u64, Gas, DeployHash);

/// Stores the internal state of the BlockProposer.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct BlockProposerDeploySets {
    /// The collection of deploys pending for inclusion in a block.
    /// The key of this map is used as a method of keeping the map sorted first by `gas_price`
    /// (willingness to pay), then by `payment_amount` and finally include `deploy_hash` to
    /// facilitate map operations.
    /// https://github.com/CasperLabs/ceps/blob/Gas_spot_market/text/0022-gas-spot-market.md#ordering
    pub(super) pending: BTreeMap<PendingKey, DeployType>,
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

impl Default for BlockProposerDeploySets {
    fn default() -> Self {
        let pending = BTreeMap::new();
        let finalized_deploys = Default::default();
        let next_finalized = Default::default();
        let finalization_queue = Default::default();
        BlockProposerDeploySets {
            pending,
            finalized_deploys,
            next_finalized,
            finalization_queue,
        }
    }
}

impl BlockProposerDeploySets {
    pub(super) fn with_next_finalized(self, next_finalized: BlockHeight) -> Self {
        BlockProposerDeploySets {
            next_finalized,
            ..self
        }
    }
}

impl Display for BlockProposerDeploySets {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "(pending:{}, finalized:{})",
            self.pending.len(),
            self.finalized_deploys.len()
        )
    }
}

/// Create a state storage key for block proposer deploy sets based on a chainspec.
///
/// We namespace based on a chainspec to prevent validators from loading data for a different chain
/// if they forget to clear their state.
pub fn create_storage_key(chainspec: &Chainspec) -> Vec<u8> {
    format!(
        "block_proposer_deploy_sets:version={},chain_name={}",
        chainspec.genesis.protocol_version, chainspec.genesis.name
    )
    .into()
}

impl BlockProposerDeploySets {
    /// Prunes expired deploy information from the BlockProposerState, returns the total deploys
    /// pruned
    pub(crate) fn prune(&mut self, current_instant: Timestamp) -> usize {
        let pending = prune_pending_deploys(&mut self.pending, current_instant);
        let finalized = prune_deploys(&mut self.finalized_deploys, current_instant);
        pending + finalized
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
    deploys: &mut BTreeMap<PendingKey, DeployType>,
    current_instant: Timestamp,
) -> usize {
    let initial_len = deploys.len();
    let remove_keys = deploys
        .iter()
        .filter_map(|(key, wrapper)| {
            if wrapper.header().expired(current_instant) {
                Some(*key)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    for key in remove_keys {
        deploys.remove(&key);
    }
    initial_len - deploys.len()
}
