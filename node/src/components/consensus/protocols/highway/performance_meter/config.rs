use crate::components::consensus::config::Config as ConsensusConfig;
use datasize::DataSize;

use serde::{Deserialize, Serialize};

/// The number of most recent blocks for which we average the max quorum in which we participated.
pub(crate) const BLOCKS_TO_CONSIDER: usize = 5;
/// The average max quorum that triggers us to slow down: with this big or smaller average max
/// quorum per `BLOCKS_TO_CONSIDER`, we increase our round length.
pub(crate) const SLOW_DOWN_THRESHOLD: f64 = 0.8;
/// The average max quorum that triggers us to speed up: with this big or larger average max quorum
/// per `BLOCKS_TO_CONSIDER`, we decrease our round length.
pub(crate) const ACCELERATION_THRESHOLD: f64 = 0.9;
/// We will try to accelerate (decrease our round length) every `ACCELERATION_PARAMETER` rounds if
/// we have a big enough average max quorum.
pub(crate) const ACCELERATION_PARAMETER: u64 = 40;

#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Config {
    pub blocks_to_consider: usize,
    pub slowdown_threshold: f64,
    pub acceleration_threshold: f64,
    pub acceleration_parameter: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            blocks_to_consider: BLOCKS_TO_CONSIDER,
            slowdown_threshold: SLOW_DOWN_THRESHOLD,
            acceleration_threshold: ACCELERATION_THRESHOLD,
            acceleration_parameter: ACCELERATION_PARAMETER,
        }
    }
}

impl From<&ConsensusConfig> for Config {
    fn from(config: &ConsensusConfig) -> Self {
        config.highway.performance_meter
    }
}
