use std::{
    error,
    fmt::{self, Display, Formatter},
    io,
    net::SocketAddr,
    result,
    time::SystemTimeError,
};

use openssl::{error::ErrorStack, ssl};
use serde::Serialize;
use thiserror::Error;
use tracing::field;

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
    /// Failed to send message.
    #[error("failed to send message")]
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
        ssl::Error,
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

/// Wraps an error to ensure it gets properly captured by tracing.
///
/// # Note
///
/// This function should be replaced once/if the tracing issue
/// https://github.com/tokio-rs/tracing/issues/1308 has been resolved, which adds a special syntax
/// for this case and the known issue https://github.com/tokio-rs/tracing/issues/1308 has been
/// fixed, which cuts traces short after the first cause.
pub(crate) fn cl_error<'a, T>(err: &'a T) -> field::DisplayValue<ErrFormatter<'a, T>>
where
    T: error::Error + 'a,
{
    field::display(ErrFormatter(err))
}

#[cfg(test)]
mod tests {
    use thiserror::Error;
    use tracing::Value;

    use super::{cl_error, ErrFormatter};

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
