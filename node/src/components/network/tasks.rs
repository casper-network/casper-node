//! Tasks run by the component.

use std::{
    fmt::Display,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
};

use futures::{
    future::{self, Either},
    pin_mut,
};

use juliet::rpc::IncomingRequest;
use openssl::{
    pkey::{PKey, Private},
    ssl::Ssl,
    x509::X509,
};
use serde::de::DeserializeOwned;
use strum::EnumCount;
use tokio::net::TcpStream;
use tokio_openssl::SslStream;
use tracing::{
    debug, error_span,
    field::{self, Empty},
    info, trace, warn, Instrument, Span,
};

use casper_types::{ProtocolVersion, TimeDiff};

use super::{
    chain_info::ChainInfo,
    conman::{ProtocolHandler, ProtocolHandshakeOutcome},
    connection_id::ConnectionId,
    error::{ConnectionError, MessageReceiverError, MessageSenderError},
    message::NodeKeyPair,
    Channel, Event, FromIncoming, Identity, Message, Metrics, Payload, RpcServer, Transport,
};

use crate::{
    components::network::{
        deserialize_network_message,
        handshake::{negotiate_handshake, HandshakeOutcome},
        Config, Ticket,
    },
    effect::{announcements::PeerBehaviorAnnouncement, requests::NetworkRequest},
    reactor::{EventQueueHandle, QueueKind},
    tls::{self, TlsCert, ValidationError},
    types::NodeId,
    utils::{display_error, LockedLineWriter, ObservableFuse, Peel},
};

/// Low-level TLS connection function.
///
/// Performs the actual TCP+TLS connection setup.
async fn tls_connect(
    context: &TlsConfiguration,
    peer_addr: SocketAddr,
) -> Result<(NodeId, Transport), ConnectionError> {
    let stream = TcpStream::connect(peer_addr)
        .await
        .map_err(ConnectionError::TcpConnection)?;

    stream
        .set_nodelay(true)
        .map_err(ConnectionError::TcpNoDelay)?;

    let mut transport = tls::create_tls_connector(
        context.our_cert.as_x509(),
        &context.secret_key,
        context.keylog.clone(),
    )
    .and_then(|connector| connector.configure())
    .and_then(|mut config| {
        config.set_verify_hostname(false);
        config.into_ssl("this-will-not-be-checked.example.com")
    })
    .and_then(|ssl| SslStream::new(ssl, stream))
    .map_err(ConnectionError::TlsInitialization)?;

    SslStream::connect(Pin::new(&mut transport))
        .await
        .map_err(ConnectionError::TlsHandshake)?;

    let peer_cert = transport
        .ssl()
        .peer_certificate()
        .ok_or(ConnectionError::NoPeerCertificate)?;

    let validated_peer_cert = context
        .validate_peer_cert(peer_cert)
        .map_err(ConnectionError::PeerCertificateInvalid)?;

    let peer_id = NodeId::from(validated_peer_cert.public_key_fingerprint());

    Ok((peer_id, transport))
}

/// A context holding all relevant information for networking communication shared across tasks.
pub(crate) struct NetworkContext<REv>
where
    REv: 'static,
{
    /// The handle to the reactor's event queue, used by incoming message handlers to put events
    /// onto the queue.
    event_queue: Option<EventQueueHandle<REv>>,
    /// TLS parameters.
    pub(super) tls_configuration: TlsConfiguration,
    /// Our own [`NodeId`].
    pub(super) our_id: NodeId,
    /// Weak reference to the networking metrics shared by all sender/receiver tasks.
    #[allow(dead_code)] // TODO: Readd once metrics are tracked again.
    net_metrics: Weak<Metrics>,
    /// Chain info extract from chainspec.
    pub(super) chain_info: ChainInfo,
    /// Optional set of signing keys, to identify as a node during handshake.
    node_key_pair: Option<NodeKeyPair>,
    /// Our own public listening address.
    public_addr: Option<SocketAddr>,
    /// Timeout for handshake completion.
    pub(super) handshake_timeout: TimeDiff,
}

impl<REv> NetworkContext<REv> {
    pub(super) fn new(
        cfg: Config,
        our_identity: Identity,
        keylog: Option<LockedLineWriter>,
        node_key_pair: Option<NodeKeyPair>,
        chain_info: ChainInfo,
        net_metrics: &Arc<Metrics>,
    ) -> Self {
        let Identity {
            secret_key,
            tls_certificate,
            network_ca,
        } = our_identity;
        let our_id = NodeId::from(tls_certificate.public_key_fingerprint());

        let tls_configuration = TlsConfiguration {
            network_ca,
            our_cert: tls_certificate,
            secret_key,
            keylog,
        };

        NetworkContext {
            our_id,
            public_addr: None,
            event_queue: None,
            tls_configuration,
            net_metrics: Arc::downgrade(net_metrics),
            chain_info,
            node_key_pair,
            handshake_timeout: cfg.handshake_timeout,
        }
    }

    pub(super) fn initialize(
        &mut self,
        our_public_addr: SocketAddr,
        event_queue: EventQueueHandle<REv>,
    ) {
        self.public_addr = Some(our_public_addr);
        self.event_queue = Some(event_queue);
    }

    /// Our own [`NodeId`].
    pub(super) fn our_id(&self) -> NodeId {
        self.our_id
    }

    /// Our own public listening address.
    pub(super) fn public_addr(&self) -> Option<SocketAddr> {
        self.public_addr
    }

    /// Chain info extract from chainspec.
    pub(super) fn chain_info(&self) -> &ChainInfo {
        &self.chain_info
    }

    pub(crate) fn node_key_pair(&self) -> Option<&NodeKeyPair> {
        self.node_key_pair.as_ref()
    }
}

/// TLS configuration data required to setup a connection.
pub(super) struct TlsConfiguration {
    /// TLS certificate authority associated with this node's identity.
    pub(super) network_ca: Option<Arc<X509>>,
    /// TLS certificate associated with this node's identity.
    pub(super) our_cert: Arc<TlsCert>,
    /// Secret key associated with `our_cert`.
    pub(super) secret_key: Arc<PKey<Private>>,
    /// Logfile to log TLS keys to. If given, automatically enables logging.
    pub(super) keylog: Option<LockedLineWriter>,
}

impl TlsConfiguration {
    pub(crate) fn validate_peer_cert(&self, peer_cert: X509) -> Result<TlsCert, ValidationError> {
        match &self.network_ca {
            Some(ca_cert) => tls::validate_cert_with_authority(peer_cert, ca_cert),
            None => tls::validate_self_signed_cert(peer_cert),
        }
    }
}
