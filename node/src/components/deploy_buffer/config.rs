use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

const DEFAULT_DEPLOY_DELAY: &str = "15sec";
// TODO: 60s might be too aggressive
const DEFAULT_EXPIRY_CHECK_INTERVAL: &str = "1min";

#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// Deploys are only proposed in a new block if they have been received at least this long ago.
    /// A longer delay makes it more likely that many proposed deploys are already known by the
    /// other nodes, and don't have to be requested from the proposer afterwards.
    deploy_delay: TimeDiff,
    /// The interval of checking for expired deploys.
    expiry_check_interval: TimeDiff,
}

impl Config {
    // todo! - use this
    // pub(crate) fn deploy_delay(&self) -> TimeDiff {
    //     self.deploy_delay
    // }

    pub(crate) fn expiry_check_interval(&self) -> TimeDiff {
        self.expiry_check_interval
    }

    #[cfg(test)]
    pub(crate) fn set_deploy_delay(&mut self, deploy_delay: TimeDiff) {
        self.deploy_delay = deploy_delay;
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            deploy_delay: DEFAULT_DEPLOY_DELAY.parse().unwrap(),
            expiry_check_interval: DEFAULT_EXPIRY_CHECK_INTERVAL.parse().unwrap(),
        }
    }
}
