use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Default binding address for the speculative execution RPC HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:1";
/// Default rate limit in qps.
const DEFAULT_QPS_LIMIT: u64 = 1;
/// Default max body bytes (2.5MB).
const DEFAULT_MAX_BODY_BYTES: u32 = 2_621_440;

/// JSON-RPC HTTP server configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Setting to enable the HTTP server.
    pub enable_server: bool,
    /// Address to bind JSON-RPC speculative execution server to.
    pub address: String,
    /// Maximum rate limit in queries per second.
    pub qps_limit: u64,
    /// Maximum number of bytes to accept in a single request body.
    pub max_body_bytes: u32,
}

impl Config {
    /// Creates a default instance for `RpcServer`.
    pub fn new() -> Self {
        Config {
            enable_server: false,
            address: DEFAULT_ADDRESS.to_string(),
            qps_limit: DEFAULT_QPS_LIMIT,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
