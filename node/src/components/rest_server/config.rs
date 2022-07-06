use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Default binding address for the REST HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";
/// Default rate limit in qps.
const DEFAULT_QPS_LIMIT: u64 = 100;

/// REST HTTP server configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Setting to enable the HTTP server.
    pub enable_server: bool,

    /// Address to bind REST HTTP server to.
    pub address: String,

    /// Max rate limit in qps.
    pub qps_limit: u64,
}

impl Config {
    /// Creates a default instance for `RestServer`.
    pub fn new() -> Self {
        Config {
            enable_server: true,
            address: DEFAULT_ADDRESS.to_string(),
            qps_limit: DEFAULT_QPS_LIMIT,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
