use std::sync::Arc;

use datasize::DataSize;
use openssl::{
    error::ErrorStack as OpenSslErrorStack,
    pkey::{PKey, Private},
    x509::X509,
};
use thiserror::Error;
use tracing::warn;

use super::{Config, IdentityConfig};
use crate::{
    tls::{self, LoadCertError, LoadSecretKeyError, TlsCert, ValidationError},
    types::NodeId,
    WithDir,
};

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("could not generate TLS certificate: {0}")]
    CouldNotGenerateTlsCertificate(OpenSslErrorStack),
    #[error(transparent)]
    ValidationError(#[from] ValidationError),
    #[error(transparent)]
    LoadCertError(#[from] LoadCertError),
    #[error(transparent)]
    LoadSecretKeyError(#[from] LoadSecretKeyError),
}

/// An ephemeral [PKey<Private>] and [TlsCert] that identifies this node
#[derive(DataSize, Debug, Clone)]
pub(crate) struct Identity {
    pub(super) secret_key: Arc<PKey<Private>>,
    pub(super) tls_certificate: Arc<TlsCert>,
    pub(super) network_ca: Option<Arc<X509>>,
}

impl Identity {
    fn new(secret_key: PKey<Private>, tls_certificate: TlsCert, network_ca: Option<X509>) -> Self {
        Self {
            secret_key: Arc::new(secret_key),
            tls_certificate: Arc::new(tls_certificate),
            network_ca: network_ca.map(Arc::new),
        }
    }

    pub(crate) fn from_config(config: WithDir<Config>) -> Result<Self, Error> {
        match &config.value().identity {
            Some(identity) => Self::from_identity_config(identity),
            None => Self::with_generated_certs(),
        }
    }

    fn from_identity_config(identity_config: &IdentityConfig) -> Result<Self, Error> {
        let not_yet_validated_x509_cert = tls::load_cert(&identity_config.tls_certificate)?;
        let secret_key = tls::load_secret_key(&identity_config.secret_key)?;
        let x509_cert = tls::tls_cert_from_x509(not_yet_validated_x509_cert)?;

        // Load a ca certificate (if present)
        let network_ca = tls::load_cert(&identity_config.ca_certificate)?;

        // A quick sanity check for the loaded cert against supplied CA.
        tls::validate_cert_with_authority(x509_cert.as_x509().clone(), &network_ca).map_err(
            |error| {
                warn!(%error, "the given node certificate is not signed by the network CA");
                Error::ValidationError(error)
            },
        )?;

        Ok(Identity::new(secret_key, x509_cert, Some(network_ca)))
    }

    pub(crate) fn with_generated_certs() -> Result<Self, Error> {
        let (not_yet_validated_x509_cert, secret_key) =
            tls::generate_node_cert().map_err(Error::CouldNotGenerateTlsCertificate)?;
        let tls_certificate = tls::validate_self_signed_cert(not_yet_validated_x509_cert)?;
        Ok(Identity::new(secret_key, tls_certificate, None))
    }
}

impl From<&Identity> for NodeId {
    fn from(identity: &Identity) -> Self {
        NodeId::from(identity.tls_certificate.public_key_fingerprint())
    }
}
