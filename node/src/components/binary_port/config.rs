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
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
