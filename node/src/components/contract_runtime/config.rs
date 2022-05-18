use datasize::DataSize;
use serde::{Deserialize, Serialize};

const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;

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
            max_query_depth: Some(DEFAULT_MAX_QUERY_DEPTH),
        }
    }
}
