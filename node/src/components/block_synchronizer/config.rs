use std::str::FromStr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_MAX_PARALLEL_TRIE_FETCHES: u32 = 5000;
const DEFAULT_PEER_REFRESH_INTERVAL: &str = "90sec";
const DEFAULT_NEED_NEXT_INTERVAL: &str = "1sec";
const DEFAULT_DISCONNECT_DISHONEST_PEERS_INTERVAL: &str = "10sec";

/// Configuration options for fetching.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    /// Maximum number of trie nodes to fetch in parallel.
    max_parallel_trie_fetches: u32,
    /// Time interval for the node to ask for refreshed peers.
    peer_refresh_interval: TimeDiff,
    /// Time interval for the node to check what the block synchronizer needs to acquire next.
    need_next_interval: TimeDiff,
    /// Time interval for recurring disconnection of dishonest peers.
    disconnect_dishonest_peers_interval: TimeDiff,
}

impl Config {
    pub(crate) fn max_parallel_trie_fetches(&self) -> u32 {
        self.max_parallel_trie_fetches
    }

    pub(crate) fn peer_refresh_interval(&self) -> TimeDiff {
        self.peer_refresh_interval
    }

    pub(crate) fn need_next_interval(&self) -> TimeDiff {
        self.need_next_interval
    }

    pub(crate) fn disconnect_dishonest_peers_interval(&self) -> TimeDiff {
        self.disconnect_dishonest_peers_interval
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_parallel_trie_fetches: DEFAULT_MAX_PARALLEL_TRIE_FETCHES,
            peer_refresh_interval: TimeDiff::from_str(DEFAULT_PEER_REFRESH_INTERVAL).unwrap(),
            need_next_interval: TimeDiff::from_str(DEFAULT_NEED_NEXT_INTERVAL).unwrap(),
            disconnect_dishonest_peers_interval: TimeDiff::from_str(
                DEFAULT_DISCONNECT_DISHONEST_PEERS_INTERVAL,
            )
            .unwrap(),
        }
    }
}
