use serde::{Deserialize, Serialize};

use datasize::DataSize;

use casper_types::TimeDiff;

use super::round_success_meter::config::Config as RSMConfig;

/// Highway-specific configuration.
/// NOTE: This is *NOT* protocol configuration that has to be the same on all nodes.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The duration for which incoming vertices with missing dependencies are kept in a queue.
    pub pending_vertex_timeout: TimeDiff,
    /// If the initial era's protocol state has not progressed for this long, restart.
    pub standstill_timeout: Option<TimeDiff>,
    /// Request the latest protocol state from a random peer periodically, with this interval.
    pub request_state_interval: Option<TimeDiff>,
    /// Log inactive or faulty validators periodically, with this interval.
    pub log_participation_interval: Option<TimeDiff>,
    /// Log synchronizer state periodically, with this interval.
    pub log_synchronizer_interval: Option<TimeDiff>,
    /// Log the size of every incoming and outgoing serialized unit.
    pub log_unit_sizes: bool,
    /// The maximum number of blocks by which execution is allowed to lag behind finalization.
    /// If it is more than that, consensus will pause, and resume once the executor has caught up.
    pub max_execution_delay: u64,
    /// The maximum number of peers we request the same vertex from in parallel.
    pub max_requests_for_vertex: usize,
    /// The maximum number of dependencies we request per validator in a batch.
    /// Limits requests per validator in panorama - in order to get a total number of
    /// requests, multiply by # of validators.
    pub max_request_batch_size: usize,
    pub round_success_meter: RSMConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pending_vertex_timeout: "10sec".parse().unwrap(),
            standstill_timeout: None,
            request_state_interval: Some("10sec".parse().unwrap()),
            log_participation_interval: Some("10sec".parse().unwrap()),
            log_synchronizer_interval: Some("5sec".parse().unwrap()),
            log_unit_sizes: false,
            max_execution_delay: 3,
            max_requests_for_vertex: 5,
            max_request_batch_size: 20,
            round_success_meter: RSMConfig::default(),
        }
    }
}
