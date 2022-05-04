use serde::{Deserialize, Serialize};

use datasize::DataSize;

use crate::types::TimeDiff;

/// `SimpleConsensus`-specific configuration.
/// *Note*: This is *not* protocol configuration that has to be the same on all nodes.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// If the initial era's protocol state has not progressed for this long, restart.
    pub standstill_timeout: Option<TimeDiff>,
    /// Request the latest protocol state from a random peer periodically, with this interval.
    pub sync_state_interval: Option<TimeDiff>,
    /// Log inactive or faulty validators periodically, with this interval.
    pub log_participation_interval: Option<TimeDiff>,
    /// The initial timeout for a proposal.
    pub proposal_timeout: TimeDiff,
    /// Incoming proposals whose timestamps lie further in the future are rejected.
    pub clock_tolerance: TimeDiff,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            standstill_timeout: None,
            sync_state_interval: Some("1sec".parse().unwrap()),
            log_participation_interval: Some("10sec".parse().unwrap()),
            proposal_timeout: "1sec".parse().unwrap(),
            clock_tolerance: "1sec".parse().unwrap(),
        }
    }
}
