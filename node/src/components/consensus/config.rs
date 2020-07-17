use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Consensus configuration.
#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to secret key file.
    pub secret_key_path: Option<PathBuf>,
}
