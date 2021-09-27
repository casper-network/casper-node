use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

/// Node fast-sync configuration.
#[derive(Default, DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,

    /// Whether to run in archival-sync mode. Archival-sync mode captures all data (blocks, deploys
    /// and global state) back to genesis.
    pub archival_sync: bool,
}
