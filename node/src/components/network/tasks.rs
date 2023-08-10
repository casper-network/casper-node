//! Tasks run by the component.

use std::{
    fmt::Display,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Weak},
};

use futures::{
    future::{self, Either},
    pin_mut,
};

use openssl::{
    pkey::{PKey, Private},
    ssl::Ssl,
    x509::X509,
};
use serde::de::DeserializeOwned;
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
    connection_id::ConnectionId,
    error::{ConnectionError, MessageReceiverError, MessageSenderError},
    event::{IncomingConnection, OutgoingConnection},
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
async fn tls_connect<REv>(
    context: &NetworkContext<REv>,
    peer_addr: SocketAddr,
) -> Result<(NodeId, Transport), ConnectionError>
where
    REv: 'static,
{
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

/// Initiates a TLS connection to a remote address.
pub(super) async fn connect_outgoing<P, REv>(
    context: Arc<NetworkContext<REv>>,
    peer_addr: SocketAddr,
) -> OutgoingConnection
where
    REv: 'static,
    P: Payload,
{
    let (peer_id, transport) = match tls_connect(&context, peer_addr).await {
        Ok(value) => value,
        Err(error) => return OutgoingConnection::FailedEarly { peer_addr, error },
    };

    // Register the `peer_id` on the [`Span`].
    Span::current().record("peer_id", &field::display(peer_id));

    if peer_id == context.our_id {
        info!("incoming loopback connection");
        return OutgoingConnection::Loopback { peer_addr };
    }

    debug!("Outgoing TLS connection established");

    // Setup connection id and framed transport.
    let connection_id = ConnectionId::from_connection(transport.ssl(), context.our_id, peer_id);

    // Negotiate the handshake, concluding the incoming connection process.
    match negotiate_handshake::<P, _>(&context, transport, connection_id).await {
        Ok(HandshakeOutcome {
            transport,
            public_addr,
            peer_consensus_public_key,
        }) => {
            if let Some(ref public_key) = peer_consensus_public_key {
                Span::current().record("consensus_key", &field::display(public_key));
            }

            if public_addr != peer_addr {
                // We don't need the `public_addr`, as we already connected, but warn anyway.
                warn!(%public_addr, %peer_addr, "peer advertises a different public address than what we connected to");
            }

            OutgoingConnection::Established {
                peer_addr,
                peer_id,
                peer_consensus_public_key,
                transport,
            }
        }
        Err(error) => OutgoingConnection::Failed {
            peer_addr,
            peer_id,
            error,
        },
    }
}

/// A context holding all relevant information for networking communication shared across tasks.
pub(crate) struct NetworkContext<REv>
where
    REv: 'static,
{
    /// The handle to the reactor's event queue, used by incoming message handlers to put events
    /// onto the queue.
    event_queue: Option<EventQueueHandle<REv>>,
    /// Our own [`NodeId`].
    our_id: NodeId,
    /// TLS certificate associated with this node's identity.
    our_cert: Arc<TlsCert>,
    /// TLS certificate authority associated with this node's identity.
    network_ca: Option<Arc<X509>>,
    /// Secret key associated with `our_cert`.
    pub(super) secret_key: Arc<PKey<Private>>,
    /// Logfile to log TLS keys to. If given, automatically enables logging.
    pub(super) keylog: Option<LockedLineWriter>,
    /// Weak reference to the networking metrics shared by all sender/receiver tasks.
    #[allow(dead_code)] // TODO: Readd once metrics are tracked again.
    net_metrics: Weak<Metrics>,
    /// Chain info extract from chainspec.
    chain_info: ChainInfo,
    /// Optional set of signing keys, to identify as a node during handshake.
    node_key_pair: Option<NodeKeyPair>,
    /// Our own public listening address.
    public_addr: Option<SocketAddr>,
    /// Timeout for handshake completion.
    pub(super) handshake_timeout: TimeDiff,
    /// The protocol version at which (or under) tarpitting is enabled.
    tarpit_version_threshold: Option<ProtocolVersion>,
    /// If tarpitting is enabled, duration for which connections should be kept open.
    tarpit_duration: TimeDiff,
    /// The chance, expressed as a number between 0.0 and 1.0, of triggering the tarpit.
    tarpit_chance: f32,
    /// Maximum number of demands allowed to be running at once. If 0, no limit is enforced.
    #[allow(dead_code)] // TODO: Readd if necessary for backpressure.
    max_in_flight_demands: usize,
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
        // Set the demand max from configuration, regarding `0` as "unlimited".
        let max_in_flight_demands = if cfg.max_in_flight_demands == 0 {
            usize::MAX
        } else {
            cfg.max_in_flight_demands as usize
        };

        let Identity {
            secret_key,
            tls_certificate,
            network_ca,
        } = our_identity;
        let our_id = NodeId::from(tls_certificate.public_key_fingerprint());

        NetworkContext {
            our_id,
            public_addr: None,
            event_queue: None,
            our_cert: tls_certificate,
            network_ca,
            secret_key,
            net_metrics: Arc::downgrade(net_metrics),
            chain_info,
            node_key_pair,
            handshake_timeout: cfg.handshake_timeout,
            tarpit_version_threshold: cfg.tarpit_version_threshold,
            tarpit_duration: cfg.tarpit_duration,
            tarpit_chance: cfg.tarpit_chance,
            max_in_flight_demands,
            keylog,
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

    pub(crate) fn validate_peer_cert(&self, peer_cert: X509) -> Result<TlsCert, ValidationError> {
        match &self.network_ca {
            Some(ca_cert) => tls::validate_cert_with_authority(peer_cert, ca_cert),
            None => tls::validate_self_signed_cert(peer_cert),
        }
    }

    pub(crate) fn network_ca(&self) -> Option<&Arc<X509>> {
        self.network_ca.as_ref()
    }

    pub(crate) fn node_key_pair(&self) -> Option<&NodeKeyPair> {
        self.node_key_pair.as_ref()
    }

    pub(crate) fn tarpit_chance(&self) -> f32 {
        self.tarpit_chance
    }

    pub(crate) fn tarpit_duration(&self) -> TimeDiff {
        self.tarpit_duration
    }

    pub(crate) fn tarpit_version_threshold(&self) -> Option<ProtocolVersion> {
        self.tarpit_version_threshold
    }
}

/// Handles an incoming connection.
///
/// Sets up a TLS stream and performs the protocol handshake.
async fn handle_incoming<P, REv>(
    context: Arc<NetworkContext<REv>>,
    stream: TcpStream,
    peer_addr: SocketAddr,
) -> IncomingConnection
where
    REv: From<Event<P>> + 'static,
    P: Payload,
{
    let (peer_id, transport) = match server_setup_tls(&context, stream).await {
        Ok(value) => value,
        Err(error) => {
            return IncomingConnection::FailedEarly { peer_addr, error };
        }
    };

    // Register the `peer_id` on the [`Span`] for logging the ID from here on out.
    Span::current().record("peer_id", &field::display(peer_id));

    if peer_id == context.our_id {
        info!("incoming loopback connection");
        return IncomingConnection::Loopback;
    }

    debug!("Incoming TLS connection established");

    // Setup connection id and framed transport.
    let connection_id = ConnectionId::from_connection(transport.ssl(), context.our_id, peer_id);

    // Negotiate the handshake, concluding the incoming connection process.
    match negotiate_handshake::<P, _>(&context, transport, connection_id).await {
        Ok(HandshakeOutcome {
            transport,
            public_addr,
            peer_consensus_public_key,
        }) => {
            if let Some(ref public_key) = peer_consensus_public_key {
                Span::current().record("consensus_key", &field::display(public_key));
            }

            IncomingConnection::Established {
                peer_addr,
                public_addr,
                peer_id,
                peer_consensus_public_key,
                transport,
            }
        }
        Err(error) => IncomingConnection::Failed {
            peer_addr,
            peer_id,
            error,
        },
    }
}

/// Server-side TLS setup.
///
/// This function groups the TLS setup into a convenient function, enabling the `?` operator.
pub(super) async fn server_setup_tls<REv>(
    context: &NetworkContext<REv>,
    stream: TcpStream,
) -> Result<(NodeId, Transport), ConnectionError> {
    let mut tls_stream = tls::create_tls_acceptor(
        context.our_cert.as_x509().as_ref(),
        context.secret_key.as_ref(),
        context.keylog.clone(),
    )
    .and_then(|ssl_acceptor| Ssl::new(ssl_acceptor.context()))
    .and_then(|ssl| SslStream::new(ssl, stream))
    .map_err(ConnectionError::TlsInitialization)?;

    SslStream::accept(Pin::new(&mut tls_stream))
        .await
        .map_err(ConnectionError::TlsHandshake)?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or(ConnectionError::NoPeerCertificate)?;

    let validated_peer_cert = context
        .validate_peer_cert(peer_cert)
        .map_err(ConnectionError::PeerCertificateInvalid)?;

    Ok((
        NodeId::from(validated_peer_cert.public_key_fingerprint()),
        tls_stream,
    ))
}

/// Runs the server core acceptor loop.
pub(super) async fn server<P, REv>(
    context: Arc<NetworkContext<REv>>,
    listener: tokio::net::TcpListener,
    shutdown_receiver: ObservableFuse,
) where
    REv: From<Event<P>> + Send,
    P: Payload,
{
    // The server task is a bit tricky, since it has to wait on incoming connections while at the
    // same time shut down if the networking component is dropped, otherwise the TCP socket will
    // stay open, preventing reuse.

    // We first create a future that never terminates, handling incoming connections:
    let accept_connections = async {
        let event_queue = context.event_queue.expect("component not initialized");
        loop {
            // We handle accept errors here, since they can be caused by a temporary resource
            // shortage or the remote side closing the connection while it is waiting in
            // the queue.
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    // The span setup here is used throughout the entire lifetime of the connection.
                    let span =
                        error_span!("incoming", %peer_addr, peer_id=Empty, consensus_key=Empty);

                    let context = context.clone();
                    let handler_span = span.clone();
                    tokio::spawn(
                        async move {
                            let incoming =
                                handle_incoming(context.clone(), stream, peer_addr).await;
                            event_queue
                                .schedule(
                                    Event::IncomingConnection {
                                        incoming: Box::new(incoming),
                                        span,
                                    },
                                    QueueKind::NetworkIncoming,
                                )
                                .await;
                        }
                        .instrument(handler_span),
                    );
                }

                // TODO: Handle resource errors gracefully.
                //       In general, two kinds of errors occur here: Local resource exhaustion,
                //       which should be handled by waiting a few milliseconds, or remote connection
                //       errors, which can be dropped immediately.
                //
                //       The code in its current state will consume 100% CPU if local resource
                //       exhaustion happens, as no distinction is made and no delay introduced.
                Err(ref err) => {
                    warn!(%context.our_id, err=display_error(err), "dropping incoming connection during accept")
                }
            }
        }
    };

    let shutdown_messages = shutdown_receiver.wait();
    pin_mut!(shutdown_messages);
    pin_mut!(accept_connections);

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match future::select(shutdown_messages, accept_connections).await {
        Either::Left(_) => info!(
            %context.our_id,
            "shutting down socket, no longer accepting incoming connections"
        ),
        Either::Right(_) => unreachable!(),
    }
}

/// Multi-channel message receiver.
pub(super) async fn multi_channel_message_receiver<REv, P>(
    context: Arc<NetworkContext<REv>>,
    mut rpc_server: RpcServer,
    shutdown: ObservableFuse,
    peer_id: NodeId,
    span: Span,
) -> Result<(), MessageReceiverError>
where
    P: DeserializeOwned + Send + Display + Payload,
    REv: From<Event<P>>
        + FromIncoming<P>
        + From<NetworkRequest<P>>
        + From<PeerBehaviorAnnouncement>
        + Send,
{
    // Core receival loop.
    loop {
        let next_item = rpc_server.next_request();

        // TODO: Get rid of shutdown fuse, we can drop the client instead?
        let wait_for_close_incoming = shutdown.wait();

        pin_mut!(next_item);
        pin_mut!(wait_for_close_incoming);

        let request = match future::select(next_item, wait_for_close_incoming)
            .await
            .peel()
        {
            Either::Left(outcome) => {
                if let Some(request) = outcome? {
                    request
                } else {
                    {
                        // Remote closed the connection.
                        return Ok(());
                    }
                }
            }
            Either::Right(()) => {
                // We were asked to shut down.
                return Ok(());
            }
        };

        let channel = Channel::from_repr(request.channel().get())
            .ok_or_else(|| MessageReceiverError::InvalidChannel(request.channel().get()))?;
        let payload = request
            .payload()
            .as_ref()
            .ok_or_else(|| MessageReceiverError::EmptyRequest)?;

        let msg: Message<P> = deserialize_network_message(payload)
            .map_err(MessageReceiverError::DeserializationError)?;

        trace!(%msg, %channel, "message received");

        // Ensure the peer did not try to sneak in a message on a different channel.
        // TODO: Verify we still need this.
        let msg_channel = msg.get_channel();
        if msg_channel != channel {
            return Err(MessageReceiverError::WrongChannel {
                got: msg_channel,
                expected: channel,
            });
        }

        let queue_kind = if msg.is_low_priority() {
            QueueKind::NetworkLowPriority
        } else {
            QueueKind::NetworkIncoming
        };

        context
            .event_queue
            .expect("TODO: What to do if event queue is missing here?")
            .schedule(
                Event::IncomingMessage {
                    peer_id: Box::new(peer_id),
                    msg: Box::new(msg),
                    span: span.clone(),
                    ticket: Ticket::from_rpc_request(request),
                },
                queue_kind,
            )
            .await;
    }
}

/// RPC sender task.
///
/// While the sending connection does not receive any messages, it is still necessary to run the
/// server portion in a loop to ensure outgoing messages are actually processed.
pub(super) async fn rpc_sender_loop(mut rpc_server: RpcServer) -> Result<(), MessageSenderError> {
    loop {
        if let Some(incoming_request) = rpc_server.next_request().await? {
            return Err(MessageSenderError::UnexpectedIncomingRequest(
                incoming_request,
            ));
        } else {
            // Connection closed regularly.
        }
    }
}
