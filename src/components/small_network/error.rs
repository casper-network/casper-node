use std::{io, result, time::SystemTimeError};

use openssl::error::ErrorStack;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_openssl::HandshakeError;

use crate::tls::ValidationError;

pub(super) type Result<T> = result::Result<T, Error>;

/// Error type returned by the `SmallNetwork` component.
#[derive(Debug, Error)]
pub enum Error {
    /// Server failed to present certificate.
    #[error("no server certificate presented")]
    NoServerCertificate,
    /// Peer failed to present certificate.
    #[error("no peer certificate presented")]
    NoPeerCertificate,
    /// Peer has wrong ID.
    #[error("remote node has wrong ID")]
    WrongId,
    /// Invalid configuration.
    #[error("need either both or none of cert, private_key in network config")]
    InvalidConfig,
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    /// OpenSSL error.
    #[error("OpenSSL error: {0}")]
    OpenSSL(#[from] ErrorStack),
    /// Handshaking error.
    #[error("handshake error: {0}")]
    Handshake(#[from] HandshakeError<TcpStream>),
    /// TLS validation error.
    #[error("TLS validation error: {0}")]
    TlsValidation(#[from] ValidationError),
    /// System time error.
    #[error("system time error: {0}")]
    SystemTime(#[from] SystemTimeError),
    /// Other error.
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}
