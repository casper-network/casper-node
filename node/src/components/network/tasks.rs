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
    transport::TlsConfiguration,
    Channel, Event, FromIncoming, Identity, Message, Metrics, Payload, RpcServer, Transport,
};

use crate::{
    components::network::{
        deserialize_network_message, handshake::HandshakeOutcome, Config, Ticket,
    },
    effect::{announcements::PeerBehaviorAnnouncement, requests::NetworkRequest},
    reactor::{EventQueueHandle, QueueKind},
    tls::{self, TlsCert, ValidationError},
    types::NodeId,
    utils::{display_error, LockedLineWriter, ObservableFuse, Peel},
};

/// A context holding all relevant information for networking communication shared across tasks.
pub(crate) struct NetworkContext {
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

impl NetworkContext {
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
            tls_configuration,
            net_metrics: Arc::downgrade(net_metrics),
            chain_info,
            node_key_pair,
            handshake_timeout: cfg.handshake_timeout,
        }
    }

    pub(super) fn initialize(&mut self, our_public_addr: SocketAddr) {
        self.public_addr = Some(our_public_addr);
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
