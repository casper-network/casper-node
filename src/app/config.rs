//! Configuration file management.
//!
//! Configuration for the node is loaded from TOML files, but all configuration values have sensible
//! defaults.
//!
//! The binary offers an option to generate a configuration from defaults for editing. I.e. running
//! the following will dump a default configuration file to stdout:
//! ```
//! cargo run --release -- generate-config
//! ```
//!
//! # Adding a configuration section
//!
//! When adding a section to the configuration, ensure that
//!
//! * it has an entry in the root configuration [`Config`](struct.Config.html),
//! * `Default` is implemented (derived or manually) with sensible defaults, and
//! * it is completely documented.

use std::{fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use casperlabs_node::Config as SmallNetworkConfig;

/// Root configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Network configuration for the validator-only network.
    pub validator_net: SmallNetworkConfig,
    /// Network configuration for the public network.
    pub public_net: SmallNetworkConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            validator_net: SmallNetworkConfig::default_on_port(34553),
            public_net: SmallNetworkConfig::default_on_port(1485),
        }
    }
}

/// Loads a TOML-formatted configuration from a given file.
pub fn load_from_file<P: AsRef<Path>>(config_path: P) -> anyhow::Result<Config> {
    let path_ref = config_path.as_ref();
    Ok(toml::from_str(
        &fs::read_to_string(path_ref)
            .with_context(|| format!("Failed to read configuration file {:?}", path_ref))?,
    )
    .with_context(|| format!("Failed to parse configuration file {:?}", path_ref))?)
}

/// Creates a TOML-formatted string from a given configuration.
pub fn to_string(cfg: &Config) -> anyhow::Result<String> {
    toml::to_string_pretty(cfg).with_context(|| "Failed to serialize default configuration")
}
