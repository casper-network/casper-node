use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_TIMESTAMP_LEEWAY: &str = "2sec";

/// Configuration options for accepting transactions.
#[derive(Copy, Clone, Serialize, Deserialize, Debug, DataSize)]
pub struct Config {
    /// The leeway allowed when considering whether a transaction is future-dated or not.
    ///
    /// To accommodate minor clock drift, transactions whose timestamps are within
    /// `timestamp_leeway` in the future are still acceptable.
    ///
    /// The maximum value to which `timestamp_leeway` can be set is defined by the chainspec
    /// setting `transactions.max_timestamp_leeway`.
    pub timestamp_leeway: TimeDiff,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timestamp_leeway: TimeDiff::from_str(DEFAULT_TIMESTAMP_LEEWAY).unwrap(),
        }
    }
}
