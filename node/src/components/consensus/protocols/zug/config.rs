use serde::{Deserialize, Serialize};

use datasize::DataSize;

use casper_types::{serde_option_time_diff, TimeDiff};

/// `Zug`-specific configuration.
/// *Note*: This is *not* protocol configuration that has to be the same on all nodes.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// If the initial era's protocol state has not progressed for this long, restart. 0 means
    /// disabled.
    #[serde(with = "serde_option_time_diff")]
    pub standstill_timeout: Option<TimeDiff>,
    /// Request the latest protocol state from a random peer periodically, with this interval. 0
    /// means disabled.
    #[serde(with = "serde_option_time_diff")]
    pub sync_state_interval: Option<TimeDiff>,
    /// Log inactive or faulty validators periodically, with this interval. 0 means disabled.
    #[serde(with = "serde_option_time_diff")]
    pub log_participation_interval: Option<TimeDiff>,
    /// The minimal and initial timeout for a proposal.
    pub proposal_timeout: TimeDiff,
    /// The additional proposal delay that is still considered fast enough, in percent. This should
    /// take into account variables like empty vs. full blocks, network traffic etc.
    /// E.g. if proposing a full block while under heavy load takes 50% longer than an empty one
    /// while idle this should be at least 50, meaning that the timeout is 50% longer than
    /// necessary for a quorum of recent proposals, approximately.
    pub proposal_grace_period: u16,
    /// The average number of rounds after which the proposal timeout adapts by a factor of 2.
    /// Note: It goes up faster than it goes down: it takes fewer rounds to double than to halve.
    pub proposal_timeout_inertia: u16,
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
            proposal_grace_period: 200,
            proposal_timeout_inertia: 10,
        }
    }
}
