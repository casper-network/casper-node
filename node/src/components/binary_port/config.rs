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
    pub allow_request_get_all_values: bool,
    /// Flag used to enable/disable the [`Trie`] request
    pub allow_request_get_trie: bool,
    /// Maximum size of a request in bytes.
    pub max_request_size_bytes: u32,
    /// Maximum size of a response in bytes.
    pub max_response_size_bytes: u32,
    /// Maximum number of in-flight requests per client.
    pub client_request_limit: u16,
    /// Number of requests that can be buffered per client.
    pub client_request_buffer_size: usize,
}

impl Config {
    /// Creates a default instance for `RpcServer`.
    pub fn new() -> Self {
        Config {
            enable_server: true,
            address: DEFAULT_ADDRESS.to_string(),
            allow_request_get_all_values: false,
            allow_request_get_trie: false,
            client_request_limit: DEFAULT_CLIENT_REQUEST_LIMIT,
            max_request_size_bytes: DEFAULT_MAX_PAYLOAD_SIZE,
            max_response_size_bytes: DEFAULT_MAX_PAYLOAD_SIZE,
            client_request_buffer_size: DEFAULT_CHANNEL_BUFFER_SIZE,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
