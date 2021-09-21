use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::{BlockHash, TimeDiff};

/// Node fast-sync configuration.
#[derive(Default, DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining. If no hash is provided, attempt to run genesis.
    pub trusted_hash: Option<BlockHash>,

    /// An (optional) time to expiration for the header for the trusted hash. If the timestamp of
    /// the trusted hash is less than the current time minus this expiration time, the node will
    /// not join the network.
    pub trusted_hash_time_to_expiration: Option<TimeDiff>,

    /// Whether to run in archival-sync mode. Archival-sync mode captures all data (blocks, deploys
    /// and global state) back to genesis.
    pub archival_sync: bool,
}
