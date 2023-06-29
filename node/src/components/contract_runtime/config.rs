use datasize::DataSize;
use serde::{Deserialize, Serialize};
use tracing::warn;

use casper_types::OS_PAGE_SIZE;

const DEFAULT_MAX_GLOBAL_STATE_SIZE: usize = 805_306_368_000; // 750 GiB
const DEFAULT_MAX_READERS: u32 = 512;
const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
const DEFAULT_MANUAL_SYNC_ENABLED: bool = true;

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
    pub max_global_state_size: Option<usize>,
    /// The maximum number of readers to use for the global state store.
    ///
    /// Defaults to 512.
    pub max_readers: Option<u32>,
    /// The limit of depth of recursive global state queries.
    ///
    /// Defaults to 5.
    pub max_query_depth: Option<u64>,
    /// Enable synchronizing to disk only after each block is written.
    ///
    /// Defaults to `true`.
    pub enable_manual_sync: Option<bool>,
}

impl Config {
    /// Max global state size in bytes.
    pub fn max_global_state_size_or_default(&self) -> usize {
        let value = self
            .max_global_state_size
            .unwrap_or(DEFAULT_MAX_GLOBAL_STATE_SIZE);
        if value % *OS_PAGE_SIZE != 0 {
            warn!(
                "maximum global state database size {} is not multiple of system page size {}",
                value, *OS_PAGE_SIZE
            );
        }
        value
    }

    /// Max lmdb readers.
    pub fn max_readers_or_default(&self) -> u32 {
        self.max_readers.unwrap_or(DEFAULT_MAX_READERS)
    }

    /// Max query depth.
    pub fn max_query_depth_or_default(&self) -> u64 {
        self.max_query_depth.unwrap_or(DEFAULT_MAX_QUERY_DEPTH)
    }

    /// Is manual sync enabled.
    pub fn manual_sync_enabled_or_default(&self) -> bool {
        self.enable_manual_sync
            .unwrap_or(DEFAULT_MANUAL_SYNC_ENABLED)
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
