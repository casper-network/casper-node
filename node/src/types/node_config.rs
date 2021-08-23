use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

/// Node fast-sync configuration.
#[derive(Default, DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining. If no hash is provided, attempt to run genesis.
    pub trusted_hash: Option<BlockHash>,

    /// Whether to run in archival-sync mode. Archival-sync mode captures all data (blocks, deploys
    /// and global state) back to genesis. Defaults to `false`.
    #[serde(default = "NodeConfig::default_archival_sync_status")]
    pub archival_sync: bool,
}

impl NodeConfig {
    fn default_archival_sync_status() -> bool {
        false
    }
}
