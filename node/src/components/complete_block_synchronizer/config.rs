use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_TIMEOUT: &str = "10mins";

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    timeout: TimeDiff,
}

impl Config {
    pub(crate) fn timeout(&self) -> TimeDiff {
        self.timeout
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: TimeDiff::from_str(DEFAULT_TIMEOUT).unwrap(),
        }
    }
}
