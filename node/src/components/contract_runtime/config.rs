use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::utils;

const DEFAULT_MAX_GLOBAL_STATE_SIZE: usize = 805_306_368_000; // 750 GiB
const DEFAULT_MAX_READERS: u32 = 512;
const DEFAULT_QUERY_DEPTH_LIMIT: u64 = 5;

/// Contract runtime configuration.
#[derive(Clone, Copy, DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The maximum size of the database to use for the global state store.
    ///
    /// Defaults to 805,306,368,000 == 750 GiB.
    ///
    /// The size should be a multiple of the OS page size.
    max_global_state_size: Option<usize>,
    /// The maximum number of readers to use for the global state store.
    ///
    /// Defaults to 512.
    max_readers: Option<u32>,
    /// The limit of depth of recursive global state queries.
    ///
    /// Defaults to 5.
    query_depth_limit: Option<u64>,
}

impl Config {
    pub(crate) fn max_global_state_size(&self) -> usize {
        let value = self
            .max_global_state_size
            .unwrap_or(DEFAULT_MAX_GLOBAL_STATE_SIZE);
        utils::check_multiple_of_page_size(value);
        value
    }

    pub(crate) fn max_readers(&self) -> u32 {
        self.max_readers.unwrap_or(DEFAULT_MAX_READERS)
    }

    pub(crate) fn query_depth_limit(&self) -> u64 {
        self.query_depth_limit.unwrap_or(DEFAULT_QUERY_DEPTH_LIMIT)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_global_state_size: Some(DEFAULT_MAX_GLOBAL_STATE_SIZE),
            max_readers: Some(DEFAULT_MAX_READERS),
            query_depth_limit: Some(DEFAULT_QUERY_DEPTH_LIMIT),
        }
    }
}
