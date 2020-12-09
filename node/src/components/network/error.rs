use std::io;

use libp2p::{core::connection::ConnectionLimit, noise::NoiseError, Multiaddr, TransportError};
use thiserror::Error;

/// Error type returned by the `Network` component.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid configuration: must have at least one known address.
    #[error("config must have at least one known address")]
    NoKnownAddress,

    /// Invalid configuration: gossip_address_interval must not be less than
    /// gossip_duplicate_cache_timeout.
    #[error(
        "gossip_address_interval must not be less than gossip_duplicate_cache_timeout so that \
        gossiped addresses are not blocked as duplicates"
    )]
    InvalidAddressGossipConfig,

    /// Signing libp2p-noise static ID keypair failed.
    #[error("signing libp2p-noise static ID keypair failed: {0}")]
    StaticKeypairSigning(NoiseError),

    /// Failed to listen.
    #[error("failed to listen on {address}: {error}")]
    Listen {
        address: Multiaddr,
        error: TransportError<io::Error>,
    },

    /// Failed to dial the given peer.
    #[error("failed to dial the peer on {address}: {error}")]
    DialPeer {
        address: Multiaddr,
        error: ConnectionLimit,
    },
}
