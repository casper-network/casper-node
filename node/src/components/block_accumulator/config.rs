use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_ATTEMPT_EXECUTION_THRESHOLD: u64 = 3;
const DEFAULT_DEAD_AIR_INTERVAL_SECS: u32 = 180;
const DEFAULT_PURGE_INTERVAL_SECS: u32 = 6 * 60 * 60; // Six hours.

/// Configuration options for the block accumulator.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    attempt_execution_threshold: u64,
    dead_air_interval: TimeDiff,
    purge_interval: TimeDiff,
}

impl Config {
    pub(crate) fn attempt_execution_threshold(&self) -> u64 {
        self.attempt_execution_threshold
    }

    pub(crate) fn dead_air_interval(&self) -> TimeDiff {
        self.dead_air_interval
    }

    pub(crate) fn purge_interval(&self) -> TimeDiff {
        self.purge_interval
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            attempt_execution_threshold: DEFAULT_ATTEMPT_EXECUTION_THRESHOLD,
            dead_air_interval: TimeDiff::from_seconds(DEFAULT_DEAD_AIR_INTERVAL_SECS),
            purge_interval: TimeDiff::from_seconds(DEFAULT_PURGE_INTERVAL_SECS),
        }
    }
}
