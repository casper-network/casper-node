use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Consensus configuration.
#[derive(Debug, Deserialize, Serialize, Default)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to secret key file.
    pub secret_key_path: PathBuf,
}
