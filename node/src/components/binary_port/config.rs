use std::str::FromStr;

use casper_types::TimeDiff;
use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_ADDRESS: &str = "0.0.0.0:0";
/// Default maximum message size.
const DEFAULT_MAX_MESSAGE_SIZE: u32 = 4 * 1024 * 1024;
/// Default maximum number of connections.
const DEFAULT_MAX_CONNECTIONS: usize = 5;
/// Default maximum number of requests per second.
const DEFAULT_QPS_LIMIT: usize = 110;
// Initial time given to a connection before it expires
const DEFAULT_INITIAL_CONNECTION_LIFETIME: &str = "10 seconds";
// Default amount of time which is given to a connection to extend it's lifetime when a valid
// [`Command::Get(GetRequest::Record)`] is sent to the node
const DEFAULT_GET_RECORD_REQUEST_TERMINATION_DELAY: &str = "0 seconds";
// Default amount of time which is given to a connection to extend it's lifetime when a valid
// [`Command::Get(GetRequest::Information)`] is sent to the node
const DEFAULT_GET_INFORMATION_REQUEST_TERMINATION_DELAY: &str = "5 seconds";
// Default amount of time which is given to a connection to extend it's lifetime when a valid
// [`Command::Get(GetRequest::State)`] is sent to the node
const DEFAULT_GET_STATE_REQUEST_TERMINATION_DELAY: &str = "0 seconds";
// Default amount of time which is given to a connection to extend it's lifetime when a valid
// [`Command::Get(GetRequest::Trie)`] is sent to the node
const DEFAULT_GET_TRIE_REQUEST_TERMINATION_DELAY: &str = "0 seconds";
// Default amount of time which is given to a connection to extend it's lifetime when a valid
// [`Command::TryAcceptTransaction`] is sent to the node
const DEFAULT_ACCEPT_TRANSACTION_REQUEST_TERMINATION_DELAY: &str = "24 seconds";
// Default amount of time which is given to a connection to extend it's lifetime when a valid
// [`Command::TrySpeculativeExec`] is sent to the node
const DEFAULT_SPECULATIVE_EXEC_REQUEST_TERMINATION_DELAY: &str = "0 seconds";
/// Default minimum rate (bytes per second) at which a transfer in progress (receiving a message
/// or sending a response) must proceed.
const DEFAULT_MIN_TRANSFER_THROUGHPUT_BYTES_PER_SECOND: u32 = 10 * 1024;
/// Default grace period at the start of each transfer (in either direction) during which
/// throughput is measured but not enforced.
const DEFAULT_THROUGHPUT_GRACE_PERIOD: &str = "1 second";

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
    /// Maximum size of the binary port message.
    pub max_message_size_bytes: u32,
    /// Maximum number of connections to the server.
    pub max_connections: usize,
    /// If `true`, a client may send any number of requests over a single TCP connection (subject
    /// to `initial_connection_lifetime` and the `*_termination_delay` settings). If `false`, the
    /// connection is closed right after the response to the first request has been sent
    /// (request -> response -> close).
    #[serde(default = "default_keep_long_lived_tcp_connections")]
    pub keep_long_lived_tcp_connections: bool,
    /// Maximum number of requests per second.
    pub qps_limit: usize,
    // Initial time given to a connection before it expires
    pub initial_connection_lifetime: TimeDiff,
    // The amount of time which is given to a connection to extend it's lifetime when a valid
    // [`Command::Get(GetRequest::Record)`] is sent to the node
    pub get_record_request_termination_delay: TimeDiff,
    // The amount of time which is given to a connection to extend it's lifetime when a valid
    // [`Command::Get(GetRequest::Information)`] is sent to the node
    pub get_information_request_termination_delay: TimeDiff,
    // The amount of time which is given to a connection to extend it's lifetime when a valid
    // [`Command::Get(GetRequest::State)`] is sent to the node
    pub get_state_request_termination_delay: TimeDiff,
    // The amount of time which is given to a connection to extend it's lifetime when a valid
    // [`Command::Get(GetRequest::Trie)`] is sent to the node
    pub get_trie_request_termination_delay: TimeDiff,
    // The amount of time which is given to a connection to extend it's lifetime when a valid
    // [`Command::TryAcceptTransaction`] is sent to the node
    pub accept_transaction_request_termination_delay: TimeDiff,
    // The amount of time which is given to a connection to extend it's lifetime when a valid
    // [`Command::TrySpeculativeExec`] is sent to the node
    pub speculative_exec_request_termination_delay: TimeDiff,
    /// Minimum rate (in bytes per second) at which a transfer in progress must proceed once the
    /// grace period has passed - applies both to a partially received message and to a response
    /// the peer must read (accept). Measured over consecutive windows of `throughput_grace_period`
    /// length. Has no effect between messages - the lifetime of a silent connection is governed
    /// by `initial_connection_lifetime` and the `*_termination_delay` settings. `0` disables the
    /// check, leaving transfers bounded only by the connection lifetime.
    #[serde(default = "default_min_transfer_throughput_bytes_per_second")]
    pub min_transfer_throughput_bytes_per_second: u32,
    /// Time, counted from the start of each transfer - receiving a message or sending a
    /// response - during which throughput is measured but the connection is never dropped
    /// (the minimum throughput is not enforced). Also the length of the window over which
    /// throughput is measured, and it determines how often the checks run (twice per grace
    /// period, at most every 250ms).
    ///
    /// This is also the tolerance for network hiccups: a small request must arrive in full within
    /// roughly the grace period of its first bytes, so a single lost TCP segment can get a remote
    /// client disconnected if the grace period is too short. The default of 1 second suits remote
    /// clients; if only clients on the same host or LAN connect (e.g. the sidecar), it can be
    /// lowered (e.g. to 200 ms) to drop slow peers sooner.
    #[serde(default = "default_throughput_grace_period")]
    pub throughput_grace_period: TimeDiff,
}

fn default_keep_long_lived_tcp_connections() -> bool {
    true
}

fn default_min_transfer_throughput_bytes_per_second() -> u32 {
    DEFAULT_MIN_TRANSFER_THROUGHPUT_BYTES_PER_SECOND
}

fn default_throughput_grace_period() -> TimeDiff {
    TimeDiff::from_str(DEFAULT_THROUGHPUT_GRACE_PERIOD).unwrap()
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
            max_message_size_bytes: DEFAULT_MAX_MESSAGE_SIZE,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            keep_long_lived_tcp_connections: default_keep_long_lived_tcp_connections(),
            qps_limit: DEFAULT_QPS_LIMIT,
            initial_connection_lifetime: TimeDiff::from_str(DEFAULT_INITIAL_CONNECTION_LIFETIME)
                .unwrap(),
            get_record_request_termination_delay: TimeDiff::from_str(
                DEFAULT_GET_RECORD_REQUEST_TERMINATION_DELAY,
            )
            .unwrap(),
            get_information_request_termination_delay: TimeDiff::from_str(
                DEFAULT_GET_INFORMATION_REQUEST_TERMINATION_DELAY,
            )
            .unwrap(),
            get_state_request_termination_delay: TimeDiff::from_str(
                DEFAULT_GET_STATE_REQUEST_TERMINATION_DELAY,
            )
            .unwrap(),
            get_trie_request_termination_delay: TimeDiff::from_str(
                DEFAULT_GET_TRIE_REQUEST_TERMINATION_DELAY,
            )
            .unwrap(),
            accept_transaction_request_termination_delay: TimeDiff::from_str(
                DEFAULT_ACCEPT_TRANSACTION_REQUEST_TERMINATION_DELAY,
            )
            .unwrap(),
            speculative_exec_request_termination_delay: TimeDiff::from_str(
                DEFAULT_SPECULATIVE_EXEC_REQUEST_TERMINATION_DELAY,
            )
            .unwrap(),
            min_transfer_throughput_bytes_per_second:
                default_min_transfer_throughput_bytes_per_second(),
            throughput_grace_period: default_throughput_grace_period(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
