use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

use casper_types::TimeDiff;

const DEFAULT_IDLE_TOLERANCE: &str = "20min";
const DEFAULT_MAX_ATTEMPTS: usize = 3;
const DEFAULT_CONTROL_LOGIC_DEFAULT_DELAY: &str = "1sec";
const DEFAULT_SHUTDOWN_FOR_UPGRADE_TIMEOUT: &str = "2min";
const DEFAULT_UPGRADE_TIMEOUT: &str = "30sec";

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

    /// Shutdown for upgrade state timeout, after which the node will upgrade regardless whether
    /// all the conditions are satisfied.
    pub shutdown_for_upgrade_timeout: TimeDiff,

    /// Maximum time a node will wait for an upgrade to commit.
    pub upgrade_timeout: TimeDiff,

    /// If true, prevents a node from shutting down if it is supposed to be a validator in the era.
    pub prevent_validator_shutdown: bool,
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
            shutdown_for_upgrade_timeout: DEFAULT_SHUTDOWN_FOR_UPGRADE_TIMEOUT.parse().unwrap(),
            upgrade_timeout: DEFAULT_UPGRADE_TIMEOUT.parse().unwrap(),
            prevent_validator_shutdown: false,
        }
    }
}
