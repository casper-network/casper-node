#[cfg(test)]
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::TimeDiff;

/// Default binding address.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:34553";

/// Default public address.
///
/// Automatically sets the port, but defaults publishing localhost as the public address.
const DEFAULT_PUBLIC_ADDRESS: &str = "127.0.0.1:0";

/// Default interval for gossiping network addresses.
const DEFAULT_GOSSIP_INTERVAL: Duration = Duration::from_secs(30);

// Default values for networking configuration:
impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: DEFAULT_BIND_ADDRESS.to_string(),
            public_address: DEFAULT_PUBLIC_ADDRESS.to_string(),
            known_addresses: Vec::new(),
            gossip_interval: DEFAULT_GOSSIP_INTERVAL,
            systemd_support: false,
            isolation_reconnect_delay: TimeDiff::from_seconds(2),
            initial_gossip_delay: TimeDiff::from_seconds(5),
            max_addr_pending_time: TimeDiff::from_seconds(60),
        }
    }
}

/// Small network configuration.
#[derive(DataSize, Debug, Clone, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address to bind to.
    pub bind_address: String,
    /// Publicly advertised address, in case the node has a different external IP.
    ///
    /// If the port is specified as `0`, it will be replaced with the actually bound port.
    pub public_address: String,
    /// Known address of a node on the network used for joining.
    pub known_addresses: Vec<String>,
    /// Interval in milliseconds used for gossiping.
    #[serde(with = "crate::utils::milliseconds")]
    pub gossip_interval: Duration,
    /// Enable systemd startup notification.
    pub systemd_support: bool,
    /// Minimum amount of time that has to pass before attempting to reconnect after isolation.
    pub isolation_reconnect_delay: TimeDiff,
    /// Initial delay before the first round of gossip.
    pub initial_gossip_delay: TimeDiff,
    /// Maximum allowed time for an address to be kept in the pending set.
    pub max_addr_pending_time: TimeDiff,
}

#[cfg(test)]
/// Reduced gossip interval for local testing.
const DEFAULT_TEST_GOSSIP_INTERVAL: Duration = Duration::from_secs(1);

#[cfg(test)]
/// Address used to bind all local testing networking to by default.
const TEST_BIND_INTERFACE: Ipv4Addr = Ipv4Addr::LOCALHOST;

#[cfg(test)]
impl Config {
    /// Construct a configuration suitable for testing with no known address that binds to a
    /// specific address.
    pub(super) fn new(bind_address: SocketAddr) -> Self {
        Config {
            bind_address: bind_address.to_string(),
            public_address: bind_address.to_string(),
            known_addresses: vec![bind_address.to_string()],
            gossip_interval: DEFAULT_TEST_GOSSIP_INTERVAL,
            systemd_support: false,
            ..Default::default()
        }
    }

    /// Constructs a `Config` suitable for use by the first node of a testnet on a single machine.
    pub(crate) fn default_local_net_first_node(bind_port: u16) -> Self {
        Config::new((TEST_BIND_INTERFACE, bind_port).into())
    }

    /// Constructs a `Config` suitable for use by a node joining a testnet on a single machine.
    pub(crate) fn default_local_net(known_peer_port: u16) -> Self {
        Config {
            bind_address: SocketAddr::from((TEST_BIND_INTERFACE, 0)).to_string(),
            public_address: SocketAddr::from((TEST_BIND_INTERFACE, 0)).to_string(),
            known_addresses: vec![
                SocketAddr::from((TEST_BIND_INTERFACE, known_peer_port)).to_string()
            ],
            gossip_interval: DEFAULT_TEST_GOSSIP_INTERVAL,
            systemd_support: false,
            ..Default::default()
        }
    }
}
