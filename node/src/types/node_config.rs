use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

use casper_types::TimeDiff;

/// Maximum number of fetch-deploy tasks per peer to run in parallel during chain synchronization,
/// by default.
const DEFAULT_MAX_PARALLEL_DEPLOY_FETCHES_PER_PEER: u32 = 10;
/// Maximum number of fetch-trie tasks per peer to run in parallel during chain synchronization, by
/// default.
const DEFAULT_MAX_PARALLEL_TRIE_FETCHES_PER_PEER: u32 = 10;
const DEFAULT_PEER_REDEMPTION_INTERVAL: u32 = 10_000;
const DEFAULT_RETRY_INTERVAL: &str = "100ms";

/// Node fast-sync configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,

    /// Maximum number of fetch-deploy tasks per peer to run in parallel during chain
    /// synchronization.
    pub max_parallel_deploy_fetches_per_peer: u32,

    /// Maximum number of fetch-trie tasks per peer to run in parallel during chain
    /// synchronization.
    pub max_parallel_trie_fetches_per_peer: u32,

    /// The duration for which to pause between retry attempts while synchronising during joining.
    pub retry_interval: TimeDiff,

    /// How many items to fetch before redeeming a random peer.
    pub sync_peer_redemption_interval: u32,

    /// Whether to run in sync-to-genesis mode which captures all data (blocks, deploys
    /// and global state) back to genesis.
    pub sync_to_genesis: bool,
}

impl Default for NodeConfig {
    fn default() -> NodeConfig {
        NodeConfig {
            trusted_hash: None,
            max_parallel_deploy_fetches_per_peer: DEFAULT_MAX_PARALLEL_DEPLOY_FETCHES_PER_PEER,
            max_parallel_trie_fetches_per_peer: DEFAULT_MAX_PARALLEL_TRIE_FETCHES_PER_PEER,
            retry_interval: DEFAULT_RETRY_INTERVAL.parse().unwrap(),
            sync_peer_redemption_interval: DEFAULT_PEER_REDEMPTION_INTERVAL,
            sync_to_genesis: false,
        }
    }
}
