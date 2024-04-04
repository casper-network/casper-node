use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_EXPIRY_CHECK_INTERVAL: &str = "1min";

#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The interval of checking for expired transactions.
    pub expiry_check_interval: TimeDiff,
}

impl Config {
    /// Returns the interval of checking for expired transactions.
    pub fn expiry_check_interval(&self) -> TimeDiff {
        self.expiry_check_interval
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            expiry_check_interval: DEFAULT_EXPIRY_CHECK_INTERVAL.parse().unwrap(),
        }
    }
}
