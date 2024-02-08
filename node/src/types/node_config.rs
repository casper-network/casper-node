use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::{BlockHash, TimeDiff};

const DEFAULT_IDLE_TOLERANCE: &str = "20min";
const DEFAULT_MAX_ATTEMPTS: usize = 3;
const DEFAULT_CONTROL_LOGIC_DEFAULT_DELAY: &str = "1sec";
const DEFAULT_SHUTDOWN_FOR_UPGRADE_TIMEOUT: &str = "2min";
const DEFAULT_UPGRADE_TIMEOUT: &str = "30sec";

/// Node sync configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum SyncHandling {
    /// Attempt to acquire all historical state back to genesis.
    Genesis,
    /// Only attempt to acquire necessary blocks to satisfy Time to Live requirements.
    #[default]
    Ttl,
    /// Don't attempt to sync historical blocks.
    NoSync,
}

impl SyncHandling {
    /// Sync to Genesis?
    pub fn is_sync_to_genesis(&self) -> bool {
        matches!(self, SyncHandling::Genesis)
    }

    /// Sync to Ttl?
    pub fn is_sync_to_ttl(&self) -> bool {
        matches!(self, SyncHandling::Ttl)
    }

    /// Don't Sync?
    pub fn is_no_sync(&self) -> bool {
        matches!(self, SyncHandling::NoSync)
    }
}

/// Node fast-sync configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,

    /// Which historical sync option?
    ///  Genesis: sync all the way back to genesis
    ///  Ttl: sync the necessary number of historical blocks to satisfy TTL requirement.
    ///  NoSync: don't attempt to get any historical records; i.e. go forward only.
    pub sync_handling: SyncHandling,

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
}

impl Default for NodeConfig {
    fn default() -> NodeConfig {
        NodeConfig {
            trusted_hash: None,
            sync_handling: SyncHandling::default(),
            idle_tolerance: DEFAULT_IDLE_TOLERANCE.parse().unwrap(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            control_logic_default_delay: DEFAULT_CONTROL_LOGIC_DEFAULT_DELAY.parse().unwrap(),
            force_resync: false,
            shutdown_for_upgrade_timeout: DEFAULT_SHUTDOWN_FOR_UPGRADE_TIMEOUT.parse().unwrap(),
            upgrade_timeout: DEFAULT_UPGRADE_TIMEOUT.parse().unwrap(),
        }
    }
}
