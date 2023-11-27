use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Default binding address for the JSON-RPC HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";
/// Default rate limit in qps.
const DEFAULT_QPS_LIMIT: u64 = 100;
/// Default max body bytes.  This is 2.5MB which should be able to accommodate the largest valid
/// JSON-RPC request, which would be an "account_put_deploy".
const DEFAULT_MAX_BODY_BYTES: u32 = 2_621_440;
/// Default CORS origin.
const DEFAULT_CORS_ORIGIN: &str = "";

/// JSON-RPC HTTP server configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Setting to enable the HTTP server.
    pub enable_server: bool,
    /// Address to bind JSON-RPC HTTP server to.
    pub address: String,
    /// Maximum rate limit in queries per second.
    pub qps_limit: u64,
    /// Maximum number of bytes to accept in a single request body.
    pub max_body_bytes: u32,
    /// CORS origin.
    pub cors_origin: String,
}

impl Config {
    /// Creates a default instance for `RpcServer`.
    pub fn new() -> Self {
        Config {
            enable_server: true,
            address: DEFAULT_ADDRESS.to_string(),
            qps_limit: DEFAULT_QPS_LIMIT,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            cors_origin: DEFAULT_CORS_ORIGIN.to_string(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}

/// Default address to connect to the node.
const DEFAULT_NODE_CONNECT_ADDRESS: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 28104);
/// Default request limit.
const DEFAULT_NODE_REQUEST_LIMIT: u16 = 3;
/// Default maximum payload size.
const DEFAULT_MAX_NODE_PAYLOAD_SIZE: u32 = 4 * 1024 * 1024;
/// Default queue buffer size.
const DEFAULT_CHANNEL_BUFFER_SIZE: usize = 16;
/// Default exponential backoff base delay.
const DEFAULT_EXPONENTIAL_BACKOFF_BASE_MS: u64 = 1000;
/// Default exponential backoff maximum delay.
const DEFAULT_EXPONENTIAL_BACKOFF_MAX_MS: u64 = 64_000;
/// Default exponential backoff coefficient.
const DEFAULT_EXPONENTIAL_BACKOFF_COEFFICIENT: u64 = 2;

/// Node client configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct NodeClientConfig {
    /// Address of the node.
    pub address: SocketAddr,
    /// Maximum number of requests to queue.
    pub request_limit: u16,
    /// Maximum size of a request in bytes.
    pub max_request_size_bytes: u32,
    /// Maximum size of a response in bytes.
    pub max_response_size_bytes: u32,
    /// Queue buffer size for the Juliet channel.
    pub queue_buffer_size: usize,
    /// Configuration for exponential backoff to be used for re-connects.
    pub exponential_backoff: ExponentialBackoffConfig,
}

impl NodeClientConfig {
    /// Creates a default instance for `NodeClientConfig`.
    pub fn new() -> Self {
        NodeClientConfig {
            address: DEFAULT_NODE_CONNECT_ADDRESS,
            request_limit: DEFAULT_NODE_REQUEST_LIMIT,
            max_request_size_bytes: DEFAULT_MAX_NODE_PAYLOAD_SIZE,
            max_response_size_bytes: DEFAULT_MAX_NODE_PAYLOAD_SIZE,
            queue_buffer_size: DEFAULT_CHANNEL_BUFFER_SIZE,
            exponential_backoff: ExponentialBackoffConfig {
                initial_delay_ms: DEFAULT_EXPONENTIAL_BACKOFF_BASE_MS,
                max_delay_ms: DEFAULT_EXPONENTIAL_BACKOFF_MAX_MS,
                coefficient: DEFAULT_EXPONENTIAL_BACKOFF_COEFFICIENT,
            },
        }
    }
}

impl Default for NodeClientConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Exponential backoff configuration for re-connects.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct ExponentialBackoffConfig {
    /// Initial wait time before the first re-connect attempt.
    pub initial_delay_ms: u64,
    /// Maximum wait time between re-connect attempts.
    pub max_delay_ms: u64,
    /// The multiplier to apply to the previous delay to get the next delay.
    pub coefficient: u64,
}
