#[cfg(test)]
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;

use datasize::DataSize;
use libp2p::request_response::RequestResponseConfig;
use serde::{Deserialize, Serialize};

use crate::{components::small_network, types::TimeDiff};

// TODO - remove these defaults once small_network's config has been replaced by this one.
mod temp {
    pub(super) const CONNECTION_SETUP_TIMEOUT: &str = "10seconds";
    // TODO - set to reasonable limit, or remove.
    pub(super) const MAX_ONE_WAY_MESSAGE_SIZE: u32 = u32::max_value();
    pub(super) const REQUEST_TIMEOUT: &str = "10seconds";
    pub(super) const CONNECTION_KEEP_ALIVE: &str = "10seconds";
    pub(super) const GOSSIP_HEARTBEAT_INTERVAL: &str = "1second";
    // TODO - set to reasonable limit, or remove.
    pub(super) const MAX_GOSSIP_MESSAGE_SIZE: u32 = u32::max_value();
    pub(super) const GOSSIP_DUPLICATE_CACHE_TIMEOUT: &str = "1minute";
}

const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:22777";
#[cfg(test)]
/// Address used to bind all local testing networking to by default.
const TEST_BIND_INTERFACE: Ipv4Addr = Ipv4Addr::LOCALHOST;

/// Peer-to-peer network configuration.
#[derive(DataSize, Debug, Clone, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind to.
    pub bind_address: String,
    /// Known address of a node on the network used for joining.
    pub known_addresses: Vec<String>,
    /// Whether this node is a bootstrap node or not.  A boostrap node will continue to run even if
    /// it has no peer connections, and is intended to be amongst the first nodes started on a
    /// network.
    pub is_bootstrap_node: bool,
    /// Enable systemd startup notification.
    pub systemd_support: bool,
    /// The timeout for connection setup (including upgrades) for all inbound and outbound
    /// connections.
    pub connection_setup_timeout: TimeDiff,
    /// The maximum serialized one-way message size in bytes.
    pub max_one_way_message_size: u32,
    /// The timeout for inbound and outbound requests.
    pub request_timeout: TimeDiff,
    /// The keep-alive timeout of idle connections.
    pub connection_keep_alive: TimeDiff,
    /// Interval used for gossip heartbeats.
    pub gossip_heartbeat_interval: TimeDiff,
    /// Maximum serialized gossip message size in bytes.
    pub max_gossip_message_size: u32,
    /// Time for which to retain a cached gossip message ID to prevent duplicates being gossiped.
    pub gossip_duplicate_cache_timeout: TimeDiff,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: DEFAULT_BIND_ADDRESS.to_string(),
            known_addresses: Vec::new(),
            is_bootstrap_node: false,
            systemd_support: false,
            connection_setup_timeout: TimeDiff::from_str(temp::CONNECTION_SETUP_TIMEOUT).unwrap(),
            max_one_way_message_size: temp::MAX_ONE_WAY_MESSAGE_SIZE,
            request_timeout: TimeDiff::from_str(temp::REQUEST_TIMEOUT).unwrap(),
            connection_keep_alive: TimeDiff::from_str(temp::CONNECTION_KEEP_ALIVE).unwrap(),
            gossip_heartbeat_interval: TimeDiff::from_str(temp::GOSSIP_HEARTBEAT_INTERVAL).unwrap(),
            max_gossip_message_size: temp::MAX_GOSSIP_MESSAGE_SIZE,
            gossip_duplicate_cache_timeout: TimeDiff::from_str(
                temp::GOSSIP_DUPLICATE_CACHE_TIMEOUT,
            )
            .unwrap(),
        }
    }
}

#[cfg(test)]
impl Config {
    /// Construct a configuration suitable for testing with no known address that binds to a
    /// specific address.
    pub(super) fn new(bind_address: SocketAddr, is_bootstrap_node: bool) -> Self {
        Config {
            bind_address: bind_address.to_string(),
            known_addresses: vec![bind_address.to_string()],
            is_bootstrap_node,
            ..Default::default()
        }
    }

    /// Constructs a `Config` suitable for use by the first node of a testnet on a single machine.
    pub(crate) fn default_local_net_first_node(bind_port: u16) -> Self {
        Config::new((TEST_BIND_INTERFACE, bind_port).into(), true)
    }

    /// Constructs a `Config` suitable for use by a node joining a testnet on a single machine.
    pub(crate) fn default_local_net(known_peer_port: u16) -> Self {
        Config {
            bind_address: SocketAddr::from((TEST_BIND_INTERFACE, 0)).to_string(),
            known_addresses: vec![
                SocketAddr::from((TEST_BIND_INTERFACE, known_peer_port)).to_string()
            ],
            ..Default::default()
        }
    }
}

impl From<&small_network::Config> for Config {
    fn from(config: &small_network::Config) -> Self {
        let public_ip = config
            .public_address
            .split(':')
            .next()
            .expect("should get IP from public_address");
        let bind_port = config
            .bind_address
            .split(':')
            .nth(1)
            .expect("should get port from bind_address");
        let public_address = format!("{}:{}", public_ip, bind_port);
        let is_bootstrap_node = config.known_addresses.contains(&public_address);
        Config {
            bind_address: config.bind_address.clone(),
            known_addresses: config.known_addresses.clone(),
            is_bootstrap_node,
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
