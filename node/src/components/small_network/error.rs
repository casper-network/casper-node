use std::{io, net::SocketAddr, result, time::SystemTimeError};

use openssl::error::ErrorStack;
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_openssl::HandshakeError;

use crate::{tls::ValidationError, utils::ResolveAddressError};

pub(super) type Result<T> = result::Result<T, Error>;

/// Error type returned by the `SmallNetwork` component.
#[derive(Debug, Error, Serialize)]
pub enum Error {
    /// Server failed to present certificate.
    #[error("no server certificate presented")]
    NoServerCertificate,
    /// Client failed to present certificate.
    #[error("no client certificate presented")]
    NoClientCertificate,
    /// Peer ID presented does not match the expected one.
    #[error("remote node has wrong ID")]
    WrongId,
    /// The config must have both or neither of certificate and secret key, and must have at least
    /// one known address.
    #[error(
        "need both or none of cert, secret_key in network config, and at least one known address"
    )]
    InvalidConfig,
    /// Our own certificate is not valid.
    #[error("own certificate invalid")]
    OwnCertificateInvalid(#[source] ValidationError),
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
    /// Failed to send message.
    // TODO: Inclusion of the cause is a workaround, we should actually be printing cause-traces
    //       when logging errors.
    #[error("failed to send message: {0}")]
    MessageNotSent(
        #[serde(skip_serializing)]
        #[source]
        io::Error,
    ),
    /// Failed to create TLS acceptor.
    #[error("failed to create acceptor")]
    AcceptorCreation(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Failed to create configuration for TLS connector.
    #[error("failed to configure connector")]
    ConnectorConfiguration(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Failed to generate node TLS certificate.
    #[error("failed to generate cert")]
    CertificateGeneration(
        #[serde(skip_serializing)]
        #[source]
        ErrorStack,
    ),
    /// Handshaking error.
    #[error("handshake error: {0}")]
    Handshake(
        #[serde(skip_serializing)]
        #[from]
        HandshakeError<TcpStream>,
    ),
    /// TLS validation error.
    #[error("TLS validation error: {0}")]
    TlsValidation(#[from] ValidationError),
    /// System time error.
    #[error("system time error: {0}")]
    SystemTime(
        #[serde(skip_serializing)]
        #[from]
        SystemTimeError,
    ),
    /// Systemd notification error
    #[error("could not interact with systemd: {0}")]
    SystemD(#[serde(skip_serializing)] io::Error),
    /// Other error.
    #[error(transparent)]
    Anyhow(
        #[serde(skip_serializing)]
        #[from]
        anyhow::Error,
    ),
    /// Server has stopped.
    #[error("failed to create outgoing connection as server has stopped")]
    ServerStopped,

    /// Instantiating metrics failed.
    #[error(transparent)]
    MetricsError(
        #[serde(skip_serializing)]
        #[from]
        prometheus::Error,
    ),
}
