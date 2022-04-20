use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::TimeDiff;

/// Block proposer configuration.
#[derive(DataSize, Debug, Deserialize, Serialize, Clone)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Deploys are only proposed in a new block if they have been received at least this long ago.
    /// A longer delay makes it more likely that many proposed deploys are already known by the
    /// other nodes, and don't have to be requested from the proposer afterwards.
    #[serde(default = "default_deploy_delay")]
    pub deploy_delay: TimeDiff,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            deploy_delay: default_deploy_delay(),
        }
    }
}

fn default_deploy_delay() -> TimeDiff {
    "1min".parse().unwrap()
}
