use datasize::DataSize;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::types::TimeDiff;

const DEFAULT_SYNC_TIMEOUT: &str = "5min";

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    sync_timeout: TimeDiff,
}

impl Config {
    pub(crate) fn get_sync_timeout(&self) -> TimeDiff {
        self.sync_timeout
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            sync_timeout: TimeDiff::from_str(DEFAULT_SYNC_TIMEOUT).unwrap(),
        }
    }
}
