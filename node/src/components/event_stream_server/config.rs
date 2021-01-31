use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Default binding address for the SSE HTTP server.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";

/// Default number of SSEs to buffer.
const DEFAULT_EVENT_STREAM_BUFFER_LENGTH: u32 = 100;

/// Default broadcast channel size.
const DEFAULT_BROADCAST_CHANNEL_SIZE: usize = 100;

/// Default rate limit in qps.
const DEFAULT_QPS_LIMIT: u64 = 100;

/// SSE HTTP server configuration.
#[derive(Clone, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind event stream SSE HTTP server to.
    pub address: String,

    /// Number of SSEs to buffer.
    pub event_stream_buffer_length: u32,

    /// The number of events to buffer in the tokio broadcast channel to help slower clients to try
    /// to avoid missing events.  See <https://docs.rs/tokio/0.2.22/tokio/sync/broadcast/index.html#lagging>
    /// for further details.
    pub broadcast_channel_size: usize,

    /// Rate limit for queries per second.
    pub qps_limit: u64,
}

impl Config {
    /// Creates a default instance for `EventStreamServer`.
    pub fn new() -> Self {
        Config {
            address: DEFAULT_ADDRESS.to_string(),
            event_stream_buffer_length: DEFAULT_EVENT_STREAM_BUFFER_LENGTH,
            broadcast_channel_size: DEFAULT_BROADCAST_CHANNEL_SIZE,
            qps_limit: DEFAULT_QPS_LIMIT,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
