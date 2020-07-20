use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_CHAINSPEC_CONFIG_PATH: &str = "chainspec.toml";

/// Node configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct NodeConfig {
    /// Path to chainspec config file.
    pub chainspec_config_path: PathBuf,
}

impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig {
            chainspec_config_path: PathBuf::from(DEFAULT_CHAINSPEC_CONFIG_PATH),
        }
    }
}
