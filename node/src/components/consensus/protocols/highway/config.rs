use serde::{Deserialize, Serialize};

use datasize::DataSize;

use casper_types::{serde_option_time_diff, TimeDiff};

use super::round_success_meter::config::Config as RSMConfig;

/// Highway-specific configuration.
/// NOTE: This is *NOT* protocol configuration that has to be the same on all nodes.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The duration for which incoming vertices with missing dependencies are kept in a queue.
    pub pending_vertex_timeout: TimeDiff,
    /// Request the latest protocol state from a random peer periodically, with this interval.
    #[serde(with = "serde_option_time_diff")]
    pub request_state_interval: Option<TimeDiff>,
    /// Log inactive or faulty validators periodically, with this interval.
    #[serde(with = "serde_option_time_diff")]
    pub log_participation_interval: Option<TimeDiff>,
    /// Log synchronizer state periodically, with this interval.
    #[serde(with = "serde_option_time_diff")]
    pub log_synchronizer_interval: Option<TimeDiff>,
    /// Log the size of every incoming and outgoing serialized unit.
    pub log_unit_sizes: bool,
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
            request_state_interval: Some("10sec".parse().unwrap()),
            log_participation_interval: Some("10sec".parse().unwrap()),
            log_synchronizer_interval: Some("5sec".parse().unwrap()),
            log_unit_sizes: false,
            max_requests_for_vertex: 5,
            max_request_batch_size: 20,
            round_success_meter: RSMConfig::default(),
        }
    }
}
