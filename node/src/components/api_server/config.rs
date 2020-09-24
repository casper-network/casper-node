use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

/// API server configuration.
#[derive(Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Interface to bind to.
    pub bind_interface: IpAddr,

    /// Port to bind to. Use 0 for a random port.
    pub bind_port: u16,

    /// Maximum number of deploys per second accepted.
    pub accepted_deploy_rate_limit: u32,

    /// Allow exceeding the deploy rate for short bursts following periods of inactivity.
    pub accepted_deploy_burst_limit: u32,
}

impl Config {
    /// Creates a default instance for `ApiServer`.
    pub fn new() -> Self {
        Config {
            bind_interface: Ipv4Addr::LOCALHOST.into(),
            bind_port: 0,
            accepted_deploy_rate_limit: 50,
            accepted_deploy_burst_limit: 1000,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
