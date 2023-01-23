use crate::components::consensus::config::Config as ConsensusConfig;
use datasize::DataSize;
use num_rational::Ratio;

use serde::{Deserialize, Serialize};

/// The number of most recent rounds we will be keeping track of.
pub(crate) const NUM_ROUNDS_TO_CONSIDER: usize = 40;
/// The number of successful rounds that triggers us to slow down: With this many or fewer
/// successes per `NUM_ROUNDS_TO_CONSIDER`, we increase our round length.
pub(crate) const NUM_ROUNDS_SLOWDOWN: usize = 10;
/// The number of successful rounds that triggers us to speed up: With this many or more successes
/// per `NUM_ROUNDS_TO_CONSIDER`, we decrease our round length.
pub(crate) const NUM_ROUNDS_SPEEDUP: usize = 32;
/// We will try to accelerate (decrease our round length) every `ACCELERATION_PARAMETER` rounds if
/// we have few enough failures.
pub(crate) const ACCELERATION_PARAMETER: u64 = 40;
/// The FTT, as a percentage (i.e. `THRESHOLD = 1` means 1% of the validators' total weight), which
/// we will use for looking for a summit in order to determine a proposal's finality.
/// The required quorum in a summit we will look for to check if a round was successful is
/// determined by this FTT.
pub(crate) const THRESHOLD: u64 = 1;

#[cfg(test)]
pub(crate) const MAX_FAILED_ROUNDS: usize = NUM_ROUNDS_TO_CONSIDER - NUM_ROUNDS_SLOWDOWN - 1;

#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Config {
    pub num_rounds_to_consider: u64,
    pub num_rounds_slowdown: u64,
    pub num_rounds_speedup: u64,
    pub acceleration_parameter: u64,
    #[data_size(skip)]
    pub acceleration_ftt: Ratio<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            num_rounds_to_consider: NUM_ROUNDS_TO_CONSIDER as u64,
            num_rounds_slowdown: NUM_ROUNDS_SLOWDOWN as u64,
            num_rounds_speedup: NUM_ROUNDS_SPEEDUP as u64,
            acceleration_parameter: ACCELERATION_PARAMETER,
            acceleration_ftt: Ratio::new(THRESHOLD, 100),
        }
    }
}

impl Config {
    /// The maximum number of failures allowed among `num_rounds_to_consider` latest rounds, with
    /// which we won't increase our round length. Exceeding this threshold will mean that we
    /// should slow down.
    pub(crate) fn max_failed_rounds(&self) -> u64 {
        self.num_rounds_to_consider
            .saturating_sub(self.num_rounds_slowdown)
            .saturating_sub(1)
    }

    /// The maximum number of failures with which we will attempt to accelerate (decrease the round
    /// exponent).
    pub(crate) fn max_failures_for_acceleration(&self) -> u64 {
        self.num_rounds_to_consider
            .saturating_sub(self.num_rounds_speedup)
    }
}

impl From<&ConsensusConfig> for Config {
    fn from(config: &ConsensusConfig) -> Self {
        config.highway.round_success_meter
    }
}
