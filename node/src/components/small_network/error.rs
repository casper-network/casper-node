use std::{
    error,
    fmt::{self, Display, Formatter},
    io,
    net::SocketAddr,
    result,
};

use datasize::DataSize;
use openssl::{error::ErrorStack, ssl};
use serde::Serialize;
use thiserror::Error;
use tracing::field;

use crate::{tls::ValidationError, utils::ResolveAddressError};

pub(super) type Result<T> = result::Result<T, Error>;

/// Error type returned by the `SmallNetwork` component.
#[derive(Debug, Error, Serialize)]
pub enum Error {
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
    MetricsError(
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

/// An error formatter.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ErrFormatter<'a, T>(pub &'a T);

impl<'a, T> Display for ErrFormatter<'a, T>
where
    T: error::Error,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut opt_source: Option<&(dyn error::Error)> = Some(self.0);

        while let Some(source) = opt_source {
            write!(f, "{}", source)?;
            opt_source = source.source();

            if opt_source.is_some() {
                f.write_str(": ")?;
            }
        }

        Ok(())
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
    /// Peer sent a non-handshake message as its first message.
    #[error("peer did not send handshake")]
    DidNotSendHandshake,
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

/// Wraps an error to ensure it gets properly captured by tracing.
///
/// # Note
///
/// This function should be replaced once/if the tracing issue
/// https://github.com/tokio-rs/tracing/issues/1308 has been resolved, which adds a special syntax
/// for this case and the known issue https://github.com/tokio-rs/tracing/issues/1308 has been
/// fixed, which cuts traces short after the first cause.
pub(crate) fn display_error<'a, T>(err: &'a T) -> field::DisplayValue<ErrFormatter<'a, T>>
where
    T: error::Error + 'a,
{
    field::display(ErrFormatter(err))
}

#[cfg(test)]
mod tests {
    use thiserror::Error;

    use super::ErrFormatter;

    #[derive(Debug, Error)]
    #[error("this is baz")]
    struct Baz;

    #[derive(Debug, Error)]
    #[error("this is bar")]
    struct Bar(#[source] Baz);

    #[derive(Debug, Error)]
    enum MyError {
        #[error("this is foo")]
        Foo {
            #[source]
            bar: Bar,
        },
    }

    #[test]
    fn test_formatter_formats_single() {
        let single = Baz;

        assert_eq!(ErrFormatter(&single).to_string().as_str(), "this is baz");
    }

    #[test]
    fn test_formatter_formats_nested() {
        let nested = MyError::Foo { bar: Bar(Baz) };

        assert_eq!(
            ErrFormatter(&nested).to_string().as_str(),
            "this is foo: this is bar: this is baz"
        );
    }
}
