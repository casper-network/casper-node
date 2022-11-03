use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

use casper_types::TimeDiff;

/// Maximum number of fetch-deploy tasks to run in parallel during chain synchronization.
const DEFAULT_MAX_PARALLEL_DEPLOY_FETCHES: u32 = 5000;
/// Maximum number of fetch-trie tasks to run in parallel during chain synchronization.
const DEFAULT_MAX_PARALLEL_TRIE_FETCHES: u32 = 5000;
const DEFAULT_MAX_PARALLEL_BLOCK_FETCHES: u32 = 50;
const DEFAULT_MAX_SYNC_FETCH_ATTEMPTS: u32 = 5;
const DEFAULT_PEER_REDEMPTION_INTERVAL: u32 = 10_000;
const DEFAULT_RETRY_INTERVAL: &str = "100ms";
const DEFAULT_IDLE_TOLERANCE: &str = "20min";
const DEFAULT_MAX_ATTEMPTS: usize = 3;
const DEFAULT_CONTROL_LOGIC_DEFAULT_DELAY: &str = "1sec";

/// Node fast-sync configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,

    /// Maximum number of deploys to fetch in parallel.
    pub max_parallel_deploy_fetches: u32,

    /// Maximum number of blocks to fetch in parallel.
    pub max_parallel_block_fetches: u32,

    /// Maximum number of trie nodes to fetch in parallel.
    pub max_parallel_trie_fetches: u32,

    /// The maximum number of retries of fetch operations during the chain synchronization process.
    /// The retry limit is in effect only when the network component reports that enough peers
    /// are connected, until that happens, the retries are unbounded.
    pub max_sync_fetch_attempts: u32,

    /// The duration for which to pause between retry attempts while synchronising during joining.
    pub retry_interval: TimeDiff,

    /// How many items to fetch before redeeming a random peer.
    pub sync_peer_redemption_interval: u32,

    /// Whether to run in sync-to-genesis mode which captures all data (blocks, deploys
    /// and global state) back to genesis.
    pub sync_to_genesis: bool,

    /// Idle time after which the syncing process is considered stalled.
    pub idle_tolerance: TimeDiff,

    /// When the syncing process is considered stalled, it'll be retried up to `max_attempts`
    /// times.
    pub max_attempts: usize,

    /// Default delay for the control events that have no dedicated delay requirements.
    pub control_logic_default_delay: TimeDiff,
}

impl Default for NodeConfig {
    fn default() -> NodeConfig {
        NodeConfig {
            trusted_hash: None,
            max_parallel_deploy_fetches: DEFAULT_MAX_PARALLEL_DEPLOY_FETCHES,
            max_parallel_block_fetches: DEFAULT_MAX_PARALLEL_BLOCK_FETCHES,
            max_parallel_trie_fetches: DEFAULT_MAX_PARALLEL_TRIE_FETCHES,
            max_sync_fetch_attempts: DEFAULT_MAX_SYNC_FETCH_ATTEMPTS,
            retry_interval: DEFAULT_RETRY_INTERVAL.parse().unwrap(),
            sync_peer_redemption_interval: DEFAULT_PEER_REDEMPTION_INTERVAL,
            idle_tolerance: DEFAULT_IDLE_TOLERANCE.parse().unwrap(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            control_logic_default_delay: DEFAULT_CONTROL_LOGIC_DEFAULT_DELAY.parse().unwrap(),
            sync_to_genesis: false,
        }
    }
}
