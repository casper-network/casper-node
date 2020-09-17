//! Complex JSON-RPC request "params" types.

use serde::Serialize;

/// Params for the RPC with method "state_get_item".
#[derive(Serialize)]
pub struct StateGetItem {
    /// The global state hash.
    global_state_hash: String,
    /// Hex-encoded `casper_types::Key`.
    key: String,
    /// The path components starting from the key as base.
    path: Vec<String>,
}

impl StateGetItem {
    pub fn new(global_state_hash: String, key: String, path: Vec<String>) -> Self {
        StateGetItem {
            global_state_hash,
            key,
            path,
        }
    }
}
