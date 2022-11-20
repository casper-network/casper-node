use std::{io, net::SocketAddr, sync::Arc};

use casper_hashing::Digest;
use casper_types::{crypto, ProtocolVersion, SecretKey};
use datasize::DataSize;
use muxink::{demux::DemultiplexerError, fragmented::DefragmentizerError};
use openssl::{error::ErrorStack, ssl};
use serde::Serialize;
use thiserror::Error;

use crate::{
    tls::{LoadCertError, ValidationError},
    utils::{LoadError, Loadable, ResolveAddressError},
};

use super::Channel;

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
    /// Could not open the specified keylog file for appending.
    #[error("could not open keylog for appending")]
    CannotAppendToKeylog(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),

    /// Instantiating metrics failed.
    #[error(transparent)]
    Metrics(
        #[serde(skip_serializing)]
        #[from]
        prometheus::Error,
    ),
    /// Failed to load a certificate.
    #[error("failed to load a certificate: {0}")]
    LoadCertificate(
        #[serde(skip_serializing)]
        #[from]
        LoadCertError,
    ),
    /// Failed to validate a network CA certificate.
    #[error("failed to validate a cert against network CA: {0:?}")]
    CertValidationError(
        #[serde(skip_serializing)]
        #[source]
        ValidationError,
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

/// An error related to the establishment of an incoming or outgoing connection.
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
    /// Did not succeed setting TCP_NODELAY on the connection.
    #[error("Could not set TCP_NODELAY on outgoing connection")]
    TcpNoDelay(
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
    HandshakeSend(#[source] RawFrameIoError),
    /// Failed to receive handshake.
    #[error("handshake receive failed")]
    HandshakeRecv(#[source] RawFrameIoError),
    /// Peer reported a network name that does not match ours.
    #[error("peer is on different network: {0}")]
    WrongNetwork(String),
    /// Peer reported an incompatible version.
    #[error("peer is running incompatible version: {0}")]
    IncompatibleVersion(ProtocolVersion),
    /// Peer is using a different chainspec.
    #[error("peer is using a different chainspec, hash: {0}")]
    WrongChainspecHash(Digest),
    /// Peer should have included the chainspec hash in the handshake message,
    /// but didn't.
    #[error("peer did not include chainspec hash in the handshake when it was required")]
    MissingChainspecHash,
    /// Peer did not send any message, or a non-handshake as its first message.
    #[error("peer did not send handshake")]
    DidNotSendHandshake,
    /// Failed to encode our handshake.
    #[error("could not encode our handshake")]
    CouldNotEncodeOurHandshake(
        #[serde(skip_serializing)]
        #[source]
        rmp_serde::encode::Error,
    ),
    /// A background sender for our handshake panicked or crashed.
    ///
    /// This is usually a bug.
    #[error("handshake sender crashed")]
    HandshakeSenderCrashed(
        #[serde(skip_serializing)]
        #[source]
        tokio::task::JoinError,
    ),
    /// Could not deserialize the message that is supposed to contain the remotes handshake.
    #[error("could not decode remote handshake message")]
    InvalidRemoteHandshakeMessage(
        #[serde(skip_serializing)]
        #[source]
        rmp_serde::decode::Error,
    ),
    /// The peer sent a consensus certificate, but it was invalid.
    #[error("invalid consensus certificate")]
    InvalidConsensusCertificate(
        #[serde(skip_serializing)]
        #[source]
        crypto::Error,
    ),
}

/// IO error sending a raw frame.
///
/// Raw frame IO is used only during the handshake, but comes with its own error conditions.
#[derive(Debug, Error, Serialize)]
pub enum RawFrameIoError {
    /// Could not send or receive the raw frame.
    #[error("io error")]
    Io(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),

    /// Length limit violation.
    #[error("advertised length of {0} exceeds configured maximum raw frame size")]
    MaximumLengthExceeded(usize),
}

/// An error produced by reading messages.
#[derive(Debug, Error)]
pub enum MessageReaderError {
    /// The semaphore that limits trie demands was closed unexpectedly.
    #[error("demand limiter semaphore closed unexpectedly")]
    UnexpectedSemaphoreClose,
    /// The message receival stack returned an error.
    // These errors can get fairly and complicated and are boxed here for that reason.
    #[error("message receive error")]
    ReceiveError(DefragmentizerError<DemultiplexerError<io::Error>>),
    /// Error deserializing message.
    #[error("message deserialization error")]
    DeserializationError(bincode::Error),
    /// Wrong channel for received message.
    #[error("received a {got} message on channel {expected}")]
    WrongChannel {
        /// The channel the message was actually received on.
        got: Channel,
        /// The channel on which the message should have been sent.
        expected: Channel,
    },
}
