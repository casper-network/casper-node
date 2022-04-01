use std::{error, io, net::SocketAddr, result, sync::Arc};

use casper_types::{ProtocolVersion, SecretKey};
use datasize::DataSize;
use openssl::{error::ErrorStack, ssl};
use serde::Serialize;
use thiserror::Error;

use crate::{
    crypto,
    tls::ValidationError,
    utils::{LoadError, Loadable, ResolveAddressError},
};

pub(super) type Result<T> = result::Result<T, Error>;

/// Error type returned by the `SmallNetwork` component.
#[derive(Debug, Error, Serialize)]
pub enum Error {
    /// We do not have any known hosts.
    #[error("could not resolve at least one known host (or none provided)")]
    EmptyKnownHosts,
    /// Consensus signing during handshake was provided, but keys could not be loaded.
    #[error("consensus keys provided, but could not be loaded: {0}")]
    LoadConsensusKeys(
        #[serde(skip_serializing)]
        #[source]
        LoadError<<Arc<SecretKey> as Loadable>::Error>,
    ),
    /// Failed to create a TCP listener.
    #[error("failed to create listener on {1}")]
    ListenerCreation(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
        SocketAddr,
    ),
    /// Failed to get TCP listener address.
    #[error("failed to get listener addr")]
    ListenerAddr(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),
    /// Failed to set listener to non-blocking.
    #[error("failed to set listener to non-blocking")]
    ListenerSetNonBlocking(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),
    /// Failed to convert std TCP listener to tokio TCP listener.
    #[error("failed to convert listener to tokio")]
    ListenerConversion(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),
    /// Could not resolve root node address.
    #[error("failed to resolve network address")]
    ResolveAddr(
        #[serde(skip_serializing)]
        #[source]
        ResolveAddressError,
    ),

    /// Instantiating metrics failed.
    #[error(transparent)]
    Metrics(
        #[serde(skip_serializing)]
        #[from]
        prometheus::Error,
    ),
}

// Manual implementation for `DataSize` - the type contains too many FFI variants that are hard to
// size, so we give up on estimating it altogether.
impl DataSize for Error {
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

impl DataSize for ConnectionError {
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

/// An error related to an incoming or outgoing connection.
#[derive(Debug, Error, Serialize)]
pub enum ConnectionError {
    /// Failed to create TLS acceptor.
    #[error("failed to create TLS acceptor/connector")]
    TlsInitialization(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// TCP connection failed.
    #[error("TCP connection failed")]
    TcpConnection(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),
    /// Handshaking error.
    #[error("TLS handshake error")]
    TlsHandshake(
        #[serde(skip_serializing)]
        #[source]
        ssl::Error,
    ),
    /// Remote failed to present a client/server certificate.
    #[error("no client certificate presented")]
    NoPeerCertificate,
    /// TLS validation error.
    #[error("TLS validation error of peer certificate")]
    PeerCertificateInvalid(#[source] ValidationError),
    /// Failed to send handshake.
    #[error("handshake send failed")]
    HandshakeSend(
        #[serde(skip_serializing)]
        #[source]
        IoError<io::Error>,
    ),
    /// Failed to receive handshake.
    #[error("handshake receive failed")]
    HandshakeRecv(
        #[serde(skip_serializing)]
        #[source]
        IoError<io::Error>,
    ),
    /// Peer reported a network name that does not match ours.
    #[error("peer is on different network: {0}")]
    WrongNetwork(String),
    /// Peer reported an incompatible version.
    #[error("peer is running incompatible version: {0}")]
    IncompatibleVersion(ProtocolVersion),
    /// Peer sent a non-handshake message as its first message.
    #[error("peer did not send handshake")]
    DidNotSendHandshake,
    /// The peer sent a consensus certificate, but it was invalid.
    #[error("invalid consensus certificate")]
    InvalidConsensusCertificate(
        #[serde(skip_serializing)]
        #[source]
        crypto::Error,
    ),
}

/// IO operation that can time out or close.
#[derive(Debug, Error)]
pub enum IoError<E>
where
    E: error::Error + 'static,
{
    /// IO operation timed out.
    #[error("io timeout")]
    Timeout,
    /// Non-timeout IO error.
    #[error(transparent)]
    Error(#[from] E),
    /// Unexpected close/end-of-file.
    #[error("closed unexpectedly")]
    UnexpectedEof,
}
