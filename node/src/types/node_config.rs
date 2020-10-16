use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{types::BlockHash, utils::External, Chainspec};

const DEFAULT_CHAINSPEC_CONFIG_PATH: &str = "chainspec.toml";

/// Node configuration.
#[derive(DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Chainspec configuration.
    pub chainspec_config_path: External<Chainspec>,
    /// Hash used as a trust anchor when joining, if any.
    pub trusted_hash: Option<BlockHash>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig {
            chainspec_config_path: External::path(DEFAULT_CHAINSPEC_CONFIG_PATH),
            trusted_hash: None,
        }
    }
}
