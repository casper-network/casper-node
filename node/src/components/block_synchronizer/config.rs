use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_TIMEOUT: &str = "10mins";
const DEFAULT_MAX_PARALLEL_TRIE_FETCHES: u32 = 5000;
const DEFAULT_PEER_REFRESH_INTERVAL: &str = "90sec";
const NEED_NEXT_INTERVAL: &str = "30ms";

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    /// Idleness timeout tolerance (if the block synchronizer is enabled but has not
    /// made progress in this specified time the node will register a failed attempt to catch up
    /// and then either try again if any reattempts remain or shut down).
    timeout: TimeDiff,
    /// Maximum number of trie nodes to fetch in parallel.
    max_parallel_trie_fetches: u32,
    /// Time interval for the node to ask for refreshed peers.
    peer_refresh_interval: TimeDiff,
    /// Time interval for the node to check what the block synchronizer needs to acquire next.
    need_next_interval: TimeDiff,
}

impl Config {
    pub(crate) fn timeout(&self) -> TimeDiff {
        self.timeout
    }

    pub(crate) fn max_parallel_trie_fetches(&self) -> u32 {
        self.max_parallel_trie_fetches
    }

    pub(crate) fn peer_refresh_interval(&self) -> TimeDiff {
        self.peer_refresh_interval
    }

    pub(crate) fn need_next_interval(&self) -> TimeDiff {
        self.need_next_interval
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            timeout: TimeDiff::from_str(DEFAULT_TIMEOUT).unwrap(),
            max_parallel_trie_fetches: DEFAULT_MAX_PARALLEL_TRIE_FETCHES,
            peer_refresh_interval: TimeDiff::from_str(DEFAULT_PEER_REFRESH_INTERVAL).unwrap(),
            need_next_interval: TimeDiff::from_str(NEED_NEXT_INTERVAL).unwrap(),
        }
    }
}
