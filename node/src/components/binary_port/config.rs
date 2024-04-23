use casper_types::{HoldBalanceHandling, DEFAULT_GAS_HOLD_BALANCE_HANDLING};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";
/// Default maximum payload size.
const DEFAULT_MAX_PAYLOAD_SIZE: u32 = 4 * 1024 * 1024;
/// Default request limit.
const DEFAULT_CLIENT_REQUEST_LIMIT: u16 = 3;
/// Default request buffer size.
const DEFAULT_CHANNEL_BUFFER_SIZE: usize = 16;
/// Default maximum number of connections.
const DEFAULT_MAX_CONNECTIONS: usize = 16;

/// Binary port server configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Setting to enable the BinaryPort server.
    pub enable_server: bool,
    /// Address to bind BinaryPort server to.
    pub address: String,
    /// Flag used to enable/disable the [`AllValues`] request
    // In case we need "enabled" flag for more than 2 requests we should introduce generic
    // "function disabled/enabled" mechanism. For now, we can stick to these two booleans.
    pub allow_request_get_all_values: bool,
    /// Flag used to enable/disable the [`Trie`] request
    pub allow_request_get_trie: bool,
    /// Flag used to enable/disable the [`TrySpeculativeExec`] request.
    pub allow_request_speculative_exec: bool,
    /// Maximum size of a request in bytes.
    pub max_request_size_bytes: u32,
    /// Maximum size of a response in bytes.
    pub max_response_size_bytes: u32,
    /// Maximum number of in-flight requests per client.
    pub client_request_limit: u16,
    /// Number of requests that can be buffered per client.
    pub client_request_buffer_size: usize,
    /// Maximum number of connections to the server.
    pub max_connections: usize,
    // Gas hold handling
    // TODO[RC]: Temporarily removed
    // pub gas_hold_handling: HoldBalanceHandling,
}

impl Config {
    /// Creates a default instance for `BinaryPort`.
    pub fn new() -> Self {
        Config {
            enable_server: true,
            address: DEFAULT_ADDRESS.to_string(),
            allow_request_get_all_values: false,
            allow_request_get_trie: false,
            allow_request_speculative_exec: false,
            client_request_limit: DEFAULT_CLIENT_REQUEST_LIMIT,
            max_request_size_bytes: DEFAULT_MAX_PAYLOAD_SIZE,
            max_response_size_bytes: DEFAULT_MAX_PAYLOAD_SIZE,
            client_request_buffer_size: DEFAULT_CHANNEL_BUFFER_SIZE,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            // TODO[RC]: Temporarily removed
            //gas_hold_handling: DEFAULT_GAS_HOLD_BALANCE_HANDLING,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
