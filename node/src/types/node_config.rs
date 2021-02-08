use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::BlockHash;

/// Node configuration.
#[derive(Default, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,
}
