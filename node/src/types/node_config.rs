use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Node configuration.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct NodeConfig {
    /// Path to chainspec file.
    pub chainspec_config_path: Option<PathBuf>,
}
