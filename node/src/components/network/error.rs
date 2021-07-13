use std::io;

use libp2p::{core::connection::ConnectionLimit, noise::NoiseError, Multiaddr, TransportError};
use thiserror::Error;

/// Error type returned by the `Network` component.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid configuration: must have at least one known address.
    #[error("config must have at least one known address")]
    NoKnownAddress,

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

    /// Failed to serialize a message.
    #[error("failed to serialize: {0}")]
    Serialization(bincode::ErrorKind),

    /// Failed to deserialize a message.
    #[error("failed to deserialize: {0}")]
    Deserialization(bincode::ErrorKind),

    /// Message too large.
    #[error("message of {actual_size} bytes exceeds limit of {max_size} bytes")]
    MessageTooLarge { max_size: u32, actual_size: u64 },

    /// Instantiating metrics failed.
    #[error(transparent)]
    Metrics(#[from] prometheus::Error),
}
