use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_CHAINSPEC_CONFIG_PATH: &str = "chainspec.toml";
const DEFAULT_BLOCK_MAX_DEPLOY_COUNT: u32 = 3;

/// Node configuration.
#[derive(Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeConfig {
    /// Path to chainspec config file.
    pub chainspec_config_path: PathBuf,
    /// The maximum number of deploys permitted in a single block.
    pub block_max_deploy_count: u32,
}

impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig {
            chainspec_config_path: PathBuf::from(DEFAULT_CHAINSPEC_CONFIG_PATH),
            block_max_deploy_count: DEFAULT_BLOCK_MAX_DEPLOY_COUNT,
        }
    }
}
