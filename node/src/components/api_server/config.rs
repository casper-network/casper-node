use std::net::{IpAddr, Ipv4Addr};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

/// API server configuration.
#[derive(DataSize, Debug, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Interface to bind to.
    pub bind_interface: IpAddr,

    /// Port to bind to. Use 0 for a random port.
    pub bind_port: u16,
}

impl Config {
    /// Creates a default instance for `ApiServer`.
    pub fn new() -> Self {
        Config {
            bind_interface: Ipv4Addr::LOCALHOST.into(),
            bind_port: 0,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new()
    }
}
