use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Default binding address for the REST HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";

/// REST HTTP server configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind REST HTTP server to.
    pub address: String,
}

impl Config {
    /// Creates a default instance for `RestServer`.
    pub fn new() -> Self {
        Config {
            address: DEFAULT_ADDRESS.to_string(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
