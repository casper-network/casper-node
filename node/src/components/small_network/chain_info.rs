//! Network-related chain identification information.

// TODO: This module and `ChainId` should disappear in its entirety and the actual chainspec be made
// available.

use std::net::SocketAddr;

use casper_types::ProtocolVersion;
use datasize::DataSize;

use super::Message;
use crate::types::Chainspec;

/// Data retained from the chainspec by the small networking component.
///
/// Typically this information is used for creating handshakes.
#[derive(DataSize, Debug)]
pub(crate) struct ChainInfo {
    /// Name of the network we participate in. We only remain connected to peers with the same
    /// network name as us.
    pub(super) network_name: String,
    /// The maximum message size for a network message, as supplied from the chainspec.
    pub(super) maximum_net_message_size: u32,
    /// The protocol version.
    pub(super) protocol_version: ProtocolVersion,
}

impl ChainInfo {
    /// Create an instance of `ChainInfo` for testing.
    #[cfg(test)]
    pub fn create_for_testing() -> Self {
        ChainInfo {
            network_name: "rust-tests-network".to_string(),
            maximum_net_message_size: 22 * 1024 * 1024, // Hardcoded at 22M.
            protocol_version: ProtocolVersion::V1_0_0,
        }
    }

    /// Create a handshake based on chain identification data.
    pub(super) fn create_handshake<P>(&self, public_address: SocketAddr) -> Message<P> {
        Message::Handshake {
            network_name: self.network_name.clone(),
            public_address,
            protocol_version: self.protocol_version,
        }
    }
}

impl From<&Chainspec> for ChainInfo {
    fn from(chainspec: &Chainspec) -> Self {
        ChainInfo {
            network_name: chainspec.network_config.name.clone(),
            maximum_net_message_size: chainspec.network_config.maximum_net_message_size,
            protocol_version: chainspec.protocol_version(),
        }
    }
}
