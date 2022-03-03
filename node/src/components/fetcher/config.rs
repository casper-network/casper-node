use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::TimeDiff;

const DEFAULT_GET_FROM_PEER_TIMEOUT: &str = "3sec";

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    get_from_peer_timeout: TimeDiff,
}

impl Config {
    pub(crate) fn get_from_peer_timeout(&self) -> TimeDiff {
        self.get_from_peer_timeout
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Unwrap is safe since string is valid
            get_from_peer_timeout: TimeDiff::from_str(DEFAULT_GET_FROM_PEER_TIMEOUT).unwrap(),
        }
    }
}
