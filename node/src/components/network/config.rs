use std::str::FromStr;

use datasize::DataSize;
use libp2p::request_response::RequestResponseConfig;
use serde::{Deserialize, Serialize};

use crate::{components::small_network, types::TimeDiff};

// TODO - remove these defaults once small_network's config has been replaced by this one.
mod temp {
    pub(super) const GOSSIP_INTERVAL: &str = "2minutes";
    pub(super) const CONNECTION_SETUP_TIMEOUT: &str = "10seconds";
    pub(super) const MAX_MESSAGE_SIZE: u32 = 1024 * 1024;
    pub(super) const REQUEST_TIMEOUT: &str = "10seconds";
    pub(super) const CONNECTION_KEEP_ALIVE: &str = "10seconds";
}

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:22777";

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: DEFAULT_BIND_ADDRESS.to_string(),
            known_addresses: Vec::new(),
            gossip_interval: TimeDiff::from_str(temp::GOSSIP_INTERVAL).unwrap(),
            systemd_support: false,
            connection_setup_timeout: TimeDiff::from_str(temp::CONNECTION_SETUP_TIMEOUT).unwrap(),
            max_message_size: temp::MAX_MESSAGE_SIZE,
            request_timeout: TimeDiff::from_str(temp::REQUEST_TIMEOUT).unwrap(),
            connection_keep_alive: TimeDiff::from_str(temp::CONNECTION_KEEP_ALIVE).unwrap(),
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
    /// Interval used for gossiping our own address.
    pub gossip_interval: TimeDiff,
    /// Enable systemd startup notification.
    pub systemd_support: bool,
    /// The timeout for connection setup (including upgrades) for all inbound and outbound
    /// connections.
    pub connection_setup_timeout: TimeDiff,
    /// The maximum serialized message size in bytes.
    pub max_message_size: u32,
    /// The timeout for inbound and outbound requests.
    pub request_timeout: TimeDiff,
    /// The keep-alive timeout of idle connections.
    pub connection_keep_alive: TimeDiff,
}

impl From<&small_network::Config> for Config {
    fn from(config: &small_network::Config) -> Self {
        let gossip_interval =
            TimeDiff::from_str(&format!("{}ms", config.gossip_interval.as_millis())).unwrap();
        Config {
            bind_address: config.bind_address.clone(),
            known_addresses: config.known_addresses.clone(),
            gossip_interval,
            systemd_support: config.systemd_support,
            ..Default::default()
        }
    }
}

impl From<&Config> for RequestResponseConfig {
    fn from(config: &Config) -> Self {
        let mut request_response_config = RequestResponseConfig::default();
        request_response_config.set_request_timeout(config.request_timeout.into());
        request_response_config.set_connection_keep_alive(config.connection_keep_alive.into());
        request_response_config
    }
}
