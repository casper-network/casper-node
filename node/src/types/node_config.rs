use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

use casper_types::TimeDiff;

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

    /// Flag which forces the node to resync all of the blocks.
    pub force_resync: bool,
}

impl Default for NodeConfig {
    fn default() -> NodeConfig {
        NodeConfig {
            trusted_hash: None,
            sync_to_genesis: false,
            idle_tolerance: DEFAULT_IDLE_TOLERANCE.parse().unwrap(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            control_logic_default_delay: DEFAULT_CONTROL_LOGIC_DEFAULT_DELAY.parse().unwrap(),
            force_resync: false,
        }
    }
}
