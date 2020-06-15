use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

/// API server configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Interface to bind to. Defaults to loopback address.
    pub bind_interface: IpAddr,

    /// Port to bind to. Use 0 for a random port. Defaults to 0.
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
