use std::io;

use libp2p::{core::connection::ConnectionLimit, noise::NoiseError, Multiaddr, TransportError};
use thiserror::Error;

/// Error type returned by the `Network` component.
#[derive(Debug, Error)]
pub enum Error {
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
