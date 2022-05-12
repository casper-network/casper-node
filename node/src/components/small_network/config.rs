#[cfg(test)]
use std::net::{Ipv4Addr, SocketAddr};

use casper_types::{ProtocolVersion, TimeDiff};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use super::EstimatorWeights;

/// Default binding address.
///
/// Uses a fixed port per node, but binds on any interface.
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:34553";

/// Default public address.
///
/// Automatically sets the port, but defaults publishing localhost as the public address.
const DEFAULT_PUBLIC_ADDRESS: &str = "127.0.0.1:0";

/// Default interval for gossiping network addresses.
const DEFAULT_GOSSIP_INTERVAL: TimeDiff = TimeDiff::from_seconds(30);

/// Default delay until initial round of address gossiping starts.
const DEFAULT_INITIAL_GOSSIP_DELAY: TimeDiff = TimeDiff::from_seconds(5);

/// Default time limit for an address to be in the pending set.
const DEFAULT_MAX_ADDR_PENDING_TIME: TimeDiff = TimeDiff::from_seconds(60);

/// Default timeout during which the handshake needs to be completed.
const DEFAULT_HANDSHAKE_TIMEOUT: TimeDiff = TimeDiff::from_seconds(20);

// Default values for networking configuration:
impl Default for Config {
    fn default() -> Self {
        Config {
            bind_address: DEFAULT_BIND_ADDRESS.to_string(),
            public_address: DEFAULT_PUBLIC_ADDRESS.to_string(),
            known_addresses: Vec::new(),
            gossip_interval: DEFAULT_GOSSIP_INTERVAL,
            initial_gossip_delay: DEFAULT_INITIAL_GOSSIP_DELAY,
            max_addr_pending_time: DEFAULT_MAX_ADDR_PENDING_TIME,
            handshake_timeout: DEFAULT_HANDSHAKE_TIMEOUT,
            max_incoming_peer_connections: 0,
            max_outgoing_byte_rate_non_validators: 0,
            max_incoming_message_rate_non_validators: 0,
            estimator_weights: Default::default(),
            reject_incompatible_versions: true,
            tarpit_version_threshold: None,
            tarpit_duration: TimeDiff::from_seconds(600),
            tarpit_chance: 0.2,
            max_in_flight_demands: 50,
            blocklist_retain_duration: TimeDiff::from_seconds(600),
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
    pub gossip_interval: TimeDiff,
    /// Initial delay before the first round of gossip.
    pub initial_gossip_delay: TimeDiff,
    /// Maximum allowed time for an address to be kept in the pending set.
    pub max_addr_pending_time: TimeDiff,
    /// Maximum allowed time for handshake completion.
    pub handshake_timeout: TimeDiff,
    /// Maximum number of incoming connections per unique peer. Unlimited if `0`.
    pub max_incoming_peer_connections: u16,
    /// Maximum number of bytes per second allowed for non-validating peers. Unlimited if 0.
    pub max_outgoing_byte_rate_non_validators: u32,
    /// Maximum of requests answered from non-validating peers. Unlimited if 0.
    pub max_incoming_message_rate_non_validators: u32,
    /// Weight distribution for the payload impact estimator.
    pub estimator_weights: EstimatorWeights,
    /// Whether or not to reject incompatible versions during handshake.
    pub reject_incompatible_versions: bool,
    /// The protocol version at which (or under) tarpitting is enabled.
    pub tarpit_version_threshold: Option<ProtocolVersion>,
    /// If tarpitting is enabled, duration for which connections should be kept open.
    pub tarpit_duration: TimeDiff,
    /// The chance, expressed as a number between 0.0 and 1.0, of triggering the tarpit.
    pub tarpit_chance: f32,
    /// Maximum number of demands for objects that can be in-flight.
    pub max_in_flight_demands: u32,
    /// Duration peers are kept on the block list, before being redeemed.
    pub blocklist_retain_duration: TimeDiff,
}

#[cfg(test)]
/// Reduced gossip interval for local testing.
const DEFAULT_TEST_GOSSIP_INTERVAL: TimeDiff = TimeDiff::from_seconds(1);

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
            ..Default::default()
        }
    }
}
