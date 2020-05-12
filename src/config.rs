//! Configuration file management
//!
//! Configuration for the node is loaded from TOML files, but all configuration values have
//! sensible defaults.
//!
//! The `cli` offers an option to generate a configuration from defaults for editing.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fs, path};

/// Root configuration
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {}

/// Loads a TOML-formatted configuration from a given file.
pub fn load_from_file<P: AsRef<path::Path>>(config_path: P) -> anyhow::Result<Config> {
    let path_ref = config_path.as_ref();
    Ok(toml::from_str(
        &fs::read_to_string(path_ref)
            .with_context(|| format!("Failed to read configuration file {:?}", path_ref))?,
    )
    .with_context(|| format!("Failed to parse configuration file {:?}", path_ref))?)
}

/// Create a TOML-formatted string from a given configuration.
pub fn to_string(cfg: &Config) -> anyhow::Result<String> {
    toml::to_string_pretty(cfg)
        .with_context(|| format!("Failed to serialize default configuration"))
}
