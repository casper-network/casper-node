use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::{BlockHash, TimeDiff};

/// Maximum number of deploys to fetch in parallel, by default.
const DEFAULT_MAX_PARALLEL_DEPLOY_FETCHES: u32 = 20;
/// Maximum number of tries to fetch in parallel, by default.
const DEFAULT_MAX_PARALLEL_TRIE_FETCHES: u32 = 20;
const DEFAULT_RETRY_INTERVAL: &str = "100ms";

/// Node fast-sync configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,

    /// Maximum number of deploys to fetch in parallel.
    pub max_parallel_deploy_fetches: u32,

    /// Maximum number of trie nodes to fetch in parallel.
    pub max_parallel_trie_fetches: u32,

    /// The duration for which to pause between retry attempts while synchronising during joining.
    pub retry_interval: TimeDiff,

    /// Whether to run in sync-to-genesis mode which captures all data (blocks, deploys
    /// and global state) back to genesis.
    pub sync_to_genesis: bool,
}

impl Default for NodeConfig {
    fn default() -> NodeConfig {
        NodeConfig {
            trusted_hash: None,
            max_parallel_deploy_fetches: DEFAULT_MAX_PARALLEL_DEPLOY_FETCHES,
            max_parallel_trie_fetches: DEFAULT_MAX_PARALLEL_TRIE_FETCHES,
            retry_interval: DEFAULT_RETRY_INTERVAL.parse().unwrap(),
            sync_to_genesis: false,
        }
    }
}
