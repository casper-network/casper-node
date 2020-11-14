use std::net::SocketAddr;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::components::small_network;

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:22777";

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: DEFAULT_BIND_ADDRESS.to_string(),
            known_addresses: Vec::new(),
            systemd_support: false,
        }
    }
}

/// Peer-to-peer network configuration.
#[derive(DataSize, Debug, Clone, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind to.
    pub bind_address: String,
    /// Known address of a node on the network used for joining.
    pub known_addresses: Vec<String>,
    /// Enable systemd startup notification.
    pub systemd_support: bool,
}

impl From<&small_network::Config> for Config {
    fn from(config: &small_network::Config) -> Self {
        let mut bind_address: SocketAddr = config
            .bind_address
            .parse()
            .expect("should parse as a SocketAddr");
        bind_address.set_port(bind_address.port().saturating_add(100));
        Config {
            bind_address: bind_address.to_string(),
            known_addresses: config.known_addresses.clone(),
            systemd_support: config.systemd_support,
        }
    }
}
