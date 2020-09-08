#[cfg(test)]
use std::net::SocketAddrV4;
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::utils;

const DEFAULT_PUBLIC_IP: Ipv4Addr = Ipv4Addr::LOCALHOST;
const DEFAULT_BIND_INTERFACE: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
const DEFAULT_LISTENING_PORT: u16 = 34_553;
const DEFAULT_GOSSIP_INTERVAL: u64 = 30_000;
#[cfg(test)]
const DEFAULT_TEST_GOSSIP_INTERVAL: u64 = 1_000;

/// Small network configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    public_ip: String,
    bind_interface: Option<String>,
    bind_port: Option<u16>,
    known_address: Option<String>,
    gossip_interval: Option<u64>,
}

impl Config {
    /// The public IP of the node.  Defaults to loopback, i.e. '127.0.0.1'.
    pub fn public_ip(&self) -> io::Result<IpAddr> {
        utils::resolve_ip(&self.public_ip)
    }

    /// Interface to bind to for listening.  Defaults to "unspecified", i.e. '0.0.0.0'.
    pub fn bind_interface(&self) -> io::Result<IpAddr> {
        self.bind_interface
            .as_deref()
            .map(utils::resolve_ip)
            .unwrap_or(Ok(DEFAULT_BIND_INTERFACE))
    }

    /// Port to bind to for listening.  Use 0 for a random port.  Defaults to 34553.
    pub fn bind_port(&self) -> u16 {
        self.bind_port.unwrap_or(DEFAULT_LISTENING_PORT)
    }

    /// Address to connect to in order to join the network.
    ///
    /// If `None`, this node will not be able to attempt to connect to the network.  Instead it will
    /// depend upon peers connecting to it.  This is normally only useful for the first node of the
    /// network.
    pub fn known_address(&self) -> Option<SocketAddr> {
        self.known_address.as_ref().map(|address| {
            utils::resolve_address(address).unwrap_or_else(|error| {
                panic!(
                    "can't parse {} as an IPv4 address and port: {}",
                    address, error
                )
            })
        })
    }

    /// The interval between each fresh round of gossiping the node's public address.  Defaults to
    /// 30 seconds.
    pub fn gossip_interval(&self) -> Duration {
        Duration::from_millis(self.gossip_interval.unwrap_or(DEFAULT_GOSSIP_INTERVAL))
    }
}

#[cfg(test)]
impl Config {
    pub(super) fn new(bind_interface: IpAddr, bind_port: u16) -> Self {
        Config {
            public_ip: DEFAULT_PUBLIC_IP.to_string(),
            bind_interface: Some(bind_interface.to_string()),
            bind_port: Some(bind_port),
            known_address: None,
            gossip_interval: Some(DEFAULT_TEST_GOSSIP_INTERVAL),
        }
    }

    /// Constructs a `Config` suitable for use by the first node of a testnet on a single machine.
    pub(crate) fn default_local_net_first_node(bind_port: u16) -> Self {
        Config {
            public_ip: DEFAULT_PUBLIC_IP.to_string(),
            bind_interface: Some(DEFAULT_BIND_INTERFACE.to_string()),
            bind_port: Some(bind_port),
            known_address: None,
            gossip_interval: Some(DEFAULT_TEST_GOSSIP_INTERVAL),
        }
    }

    /// Constructs a `Config` suitable for use by a node joining a testnet on a single machine.
    pub(crate) fn default_local_net(known_peer_port: u16) -> Self {
        Config {
            public_ip: DEFAULT_PUBLIC_IP.to_string(),
            bind_interface: Some(DEFAULT_BIND_INTERFACE.to_string()),
            bind_port: Some(0),
            known_address: Some(
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, known_peer_port).to_string(),
            ),
            gossip_interval: Some(DEFAULT_TEST_GOSSIP_INTERVAL),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            public_ip: DEFAULT_PUBLIC_IP.to_string(),
            bind_interface: Some(DEFAULT_BIND_INTERFACE.to_string()),
            bind_port: Some(DEFAULT_LISTENING_PORT),
            known_address: None,
            gossip_interval: Some(DEFAULT_GOSSIP_INTERVAL),
        }
    }
}
