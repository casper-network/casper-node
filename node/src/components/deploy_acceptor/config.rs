use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_TIMESTAMP_LEEWAY: &str = "2sec";

/// Configuration options for accepting deploys.
#[derive(Copy, Clone, Serialize, Deserialize, Debug, DataSize)]
pub struct Config {
    /// The leeway allowed when considering whether a deploy is future-dated or not.
    ///
    /// To accommodate minor clock drift, deploys whose timestamps are within `timestamp_leeway` in
    /// the future are still acceptable.
    ///
    /// The maximum value to which `timestamp_leeway` can be set is defined by the chainspec
    /// setting `deploys.max_timestamp_leeway`.
    pub timestamp_leeway: TimeDiff,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timestamp_leeway: TimeDiff::from_str(DEFAULT_TIMESTAMP_LEEWAY).unwrap(),
        }
    }
}
