use datasize::DataSize;
use serde::{Deserialize, Serialize};

const DEFAULT_GET_FROM_PEER_TIMEOUT_SECS: u64 = 10;

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    get_from_peer_timeout: u64,
}

impl Config {
    pub(crate) fn get_from_peer_timeout(&self) -> u64 {
        self.get_from_peer_timeout
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            get_from_peer_timeout: DEFAULT_GET_FROM_PEER_TIMEOUT_SECS,
        }
    }
}
