use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use datasize::DataSize;

use crate::types::TimeDiff;

use super::round_success_meter::config::Config as RSMConfig;

/// Highway-specific configuration.
/// NOTE: This is *NOT* protocol configuration that has to be the same on all nodes.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the folder where unit hash files will be stored.
    pub unit_hashes_folder: PathBuf,
    /// The duration for which incoming vertices with missing dependencies are kept in a queue.
    pub pending_vertex_timeout: TimeDiff,
    /// If the current era's protocol state has not progressed for this long, request the latest
    /// state from peers.
    pub standstill_timeout: TimeDiff,
    /// If another `standstill_timeout` passes assume we failed to join the network and restart.
    #[serde(default = "default_shutdown_on_standstill")]
    pub shutdown_on_standstill: bool,
    /// Log inactive or faulty validators periodically, with this interval.
    pub log_participation_interval: TimeDiff,
    /// Log the size of every incoming and outgoing serialized unit.
    pub log_unit_sizes: bool,
    /// The maximum number of blocks by which execution is allowed to lag behind finalization.
    /// If it is more than that, consensus will pause, and resume once the executor has caught up.
    pub max_execution_delay: u64,
    /// The maximum number of peers we request the same vertex from in parallel.
    pub max_requests_for_vertex: usize,
    pub round_success_meter: RSMConfig,
}

fn default_shutdown_on_standstill() -> bool {
    false
}

impl Default for Config {
    fn default() -> Self {
        Config {
            unit_hashes_folder: Default::default(),
            pending_vertex_timeout: "10sec".parse().unwrap(),
            standstill_timeout: "1min".parse().unwrap(),
            shutdown_on_standstill: false,
            log_participation_interval: "10sec".parse().unwrap(),
            log_unit_sizes: false,
            max_execution_delay: 3,
            max_requests_for_vertex: 5,
            round_success_meter: RSMConfig::default(),
        }
    }
}
