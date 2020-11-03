use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Default binding address for the HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:7777";

/// Default number of SSEs to buffer.
const DEFAULT_EVENT_STREAM_BUFFER_LENGTH: u32 = 100;

/// API server configuration.
#[derive(DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind HTTP server to.
    pub address: String,

    /// Number of SSEs to buffer.
    pub event_stream_buffer_length: u32,
}

impl Config {
    /// Creates a default instance for `RpcServer`.
    pub fn new() -> Self {
        Config {
            address: DEFAULT_ADDRESS.to_string(),
            event_stream_buffer_length: DEFAULT_EVENT_STREAM_BUFFER_LENGTH,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
