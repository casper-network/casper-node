use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::utils;

const DEFAULT_MAX_GLOBAL_STATE_SIZE: usize = 805_306_368_000; // 750 GiB
const DEFAULT_MAX_READERS: u32 = 512;
const DEFAULT_MAX_QUERY_DEPTH: u64 = 5;
const DEFAULT_GROW_SIZE_THRESHOLD: usize = 644_245_094_400; // 712.5 GiB (95% of default initial size)
const DEFAULT_GROW_SIZE_BYTES: usize = 53_687_091_200; // 50 GiB

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
    max_query_depth: Option<u64>,
    /// Enable synchronizing to disk only after each block is written.
    ///
    /// Defaults to `false`.
    enable_manual_sync: Option<bool>,
    /// Threshold for global state size that will trigger a resize upon next write transaction.
    ///
    /// Defaults to 644,245,094,400 == 712.5 GiB
    grow_size_threshold: Option<usize>,
    /// After global state will exceed `grow_size_threshold` bytes it will be resized by adding
    /// extra space. Defaults to 53,687,091,200 == 50 GiB
    grow_size_bytes: Option<usize>,
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

    pub(crate) fn max_query_depth(&self) -> u64 {
        self.max_query_depth.unwrap_or(DEFAULT_MAX_QUERY_DEPTH)
    }

    pub(crate) fn manual_sync_enabled(&self) -> bool {
        self.enable_manual_sync.unwrap_or(false)
    }

    pub(crate) fn grow_size_threshold(&self) -> usize {
        self.grow_size_threshold
            .unwrap_or(DEFAULT_GROW_SIZE_THRESHOLD)
    }

    pub(crate) fn grow_size_bytes(&self) -> usize {
        self.grow_size_bytes.unwrap_or(DEFAULT_GROW_SIZE_BYTES)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_global_state_size: Some(DEFAULT_MAX_GLOBAL_STATE_SIZE),
            max_readers: Some(DEFAULT_MAX_READERS),
            max_query_depth: Some(DEFAULT_MAX_QUERY_DEPTH),
            enable_manual_sync: Some(false),
            grow_size_threshold: Some(DEFAULT_GROW_SIZE_THRESHOLD),
            grow_size_bytes: Some(DEFAULT_GROW_SIZE_BYTES),
        }
    }
}
