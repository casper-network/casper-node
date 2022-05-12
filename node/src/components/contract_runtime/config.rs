use datasize::DataSize;
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_GLOBAL_STATE_SIZE: usize = 805_306_368_000; // 750 GiB
const DEFAULT_MAX_READERS: u32 = 512;
const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
const DEFAULT_MANUAL_SYNC_ENABLED: bool = true;

/// Contract runtime configuration.
#[derive(Clone, Copy, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The limit of depth of recursive global state queries.
    ///
    /// Defaults to 5.
    max_query_depth: Option<u64>,
}

impl Config {
    pub(crate) fn max_query_depth(&self) -> u64 {
        self.max_query_depth.unwrap_or(DEFAULT_MAX_QUERY_DEPTH)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_global_state_size: Some(DEFAULT_MAX_GLOBAL_STATE_SIZE),
            max_readers: Some(DEFAULT_MAX_READERS),
            max_query_depth: Some(DEFAULT_MAX_QUERY_DEPTH),
            enable_manual_sync: Some(DEFAULT_MANUAL_SYNC_ENABLED),
        }
    }
}
