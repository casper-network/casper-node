//! Tasks run by the component.

use std::{
    error::Error as StdError,
    fmt::Display,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Weak},
    time::Duration,
};

use casper_types::{ProtocolVersion, PublicKey};
use futures::{
    future::{self, Either},
    stream::{SplitSink, SplitStream},
    Future, SinkExt, StreamExt,
};
use openssl::{
    pkey::{PKey, Private},
    ssl::Ssl,
};
use prometheus::IntGauge;
use rand::Rng;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::{
    net::TcpStream,
    sync::{mpsc::UnboundedReceiver, watch},
};
use tokio_openssl::SslStream;
use tracing::{
    debug, error_span,
    field::{self, Empty},
    info, trace, warn, Instrument, Span,
};

use super::{
    chain_info::ChainInfo,
    counting_format::{ConnectionId, Role},
    error::{ConnectionError, IoError},
    event::{IncomingConnection, OutgoingConnection},
    framed,
    limiter::LimiterHandle,
    message::{ConsensusKeyPair, EstimatorWeights},
    Event, FramedTransport, Message, Metrics, Payload, Transport,
};
use crate::{
    reactor::{EventQueueHandle, QueueKind},
    tls::{self, TlsCert},
    types::{NodeId, TimeDiff},
    utils::display_error,
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

    let mut transport = tls::create_tls_connector(context.our_cert.as_x509(), &context.secret_key)
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

    let peer_id = NodeId::from(
        tls::validate_cert(peer_cert)
            .map_err(ConnectionError::PeerCertificateInvalid)?
            .public_key_fingerprint(),
    );

    Ok((peer_id, transport))
}

/// Initiates a TLS connection to a remote address.
pub(super) async fn connect_outgoing<P, REv>(
    context: Arc<NetworkContext<REv>>,
    peer_addr: SocketAddr,
) -> OutgoingConnection<P>
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

    // Setup connection sink and stream.
    let connection_id = ConnectionId::from_connection(transport.ssl(), context.our_id, peer_id);
    let mut transport = framed::<P>(
        context.net_metrics.clone(),
        connection_id,
        transport,
        Role::Dialer,
        context.chain_info.maximum_net_message_size,
    );

    // Negotiate the handshake, concluding the incoming connection process.
    match negotiate_handshake(&context, &mut transport, connection_id).await {
        Ok((public_addr, peer_consensus_public_key)) => {
            if let Some(ref public_key) = peer_consensus_public_key {
                Span::current().record("validator_id", &field::display(public_key));
            }

            if public_addr != peer_addr {
                // We don't need the `public_addr`, as we already connected, but warn anyway.
                warn!(%public_addr, %peer_addr, "peer advertises a different public address than what we connected to");
            }

            // Close the receiving end of the transport.
            let (sink, _stream) = transport.split();

            OutgoingConnection::Established {
                peer_addr,
                peer_id,
                peer_consensus_public_key,
                sink,
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
    /// Event queue handle.
    pub(super) event_queue: EventQueueHandle<REv>,
    /// Our own [`NodeId`].
    pub(super) our_id: NodeId,
    /// TLS certificate associated with this node's identity.
    pub(super) our_cert: Arc<TlsCert>,
    /// Secret key associated with `our_cert`.
    pub(super) secret_key: Arc<PKey<Private>>,
    /// Weak reference to the networking metrics shared by all sender/receiver tasks.
    pub(super) net_metrics: Weak<Metrics>,
    /// Chain info extract from chainspec.
    pub(super) chain_info: ChainInfo,
    /// Our own public listening address.
    pub(super) public_addr: SocketAddr,
    /// Optional set of consensus keys, to identify as a validator during handshake.
    pub(super) consensus_keys: Option<ConsensusKeyPair>,
    /// Timeout for handshake completion.
    pub(super) handshake_timeout: TimeDiff,
    /// Weights to estimate payloads with.
    pub(super) payload_weights: EstimatorWeights,
    /// Whether or not to reject incompatible versions during handshake.
    pub(super) reject_incompatible_versions: bool,
    /// The protocol version at which (or under) tarpitting is enabled.
    pub(super) tarpit_version_threshold: Option<ProtocolVersion>,
    /// If tarpitting is enabled, duration for which connections should be kept open.
    pub(super) tarpit_duration: TimeDiff,
    /// The chance, expressed as a number between 0.0 and 1.0, of triggering the tarpit.
    pub(super) tarpit_chance: f32,
}

/// Handles an incoming connection.
///
/// Sets up a TLS stream and performs the protocol handshake.
async fn handle_incoming<P, REv>(
    context: Arc<NetworkContext<REv>>,
    stream: TcpStream,
    peer_addr: SocketAddr,
) -> IncomingConnection<P>
where
    REv: From<Event<P>> + 'static,
    P: Payload,
    for<'de> P: Serialize + Deserialize<'de>,
    for<'de> Message<P>: Serialize + Deserialize<'de>,
{
    let (peer_id, transport) =
        match server_setup_tls(stream, &context.our_cert, &context.secret_key).await {
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

    // Setup connection sink and stream.
    let connection_id = ConnectionId::from_connection(transport.ssl(), context.our_id, peer_id);
    let mut transport = framed::<P>(
        context.net_metrics.clone(),
        connection_id,
        transport,
        Role::Listener,
        context.chain_info.maximum_net_message_size,
    );

    // Negotiate the handshake, concluding the incoming connection process.
    match negotiate_handshake(&context, &mut transport, connection_id).await {
        Ok((public_addr, peer_consensus_public_key)) => {
            if let Some(ref public_key) = peer_consensus_public_key {
                Span::current().record("validator_id", &field::display(public_key));
            }

            // Close the receiving end of the transport.
            let (_sink, stream) = transport.split();

            IncomingConnection::Established {
                peer_addr,
                public_addr,
                peer_id,
                peer_consensus_public_key,
                stream,
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
pub(super) async fn server_setup_tls(
    stream: TcpStream,
    cert: &TlsCert,
    secret_key: &PKey<Private>,
) -> Result<(NodeId, Transport), ConnectionError> {
    let mut tls_stream = tls::create_tls_acceptor(cert.as_x509().as_ref(), secret_key.as_ref())
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

    Ok((
        NodeId::from(
            tls::validate_cert(peer_cert)
                .map_err(ConnectionError::PeerCertificateInvalid)?
                .public_key_fingerprint(),
        ),
        tls_stream,
    ))
}

/// Performs an IO-operation that can time out.
async fn io_timeout<F, T, E>(duration: Duration, future: F) -> Result<T, IoError<E>>
where
    F: Future<Output = Result<T, E>>,
    E: StdError + 'static,
{
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_elapsed| IoError::Timeout)?
        .map_err(IoError::Error)
}

/// Performs an IO-operation that can time out or result in a closed connection.
async fn io_opt_timeout<F, T, E>(duration: Duration, future: F) -> Result<T, IoError<E>>
where
    F: Future<Output = Option<Result<T, E>>>,
    E: StdError + 'static,
{
    let item = tokio::time::timeout(duration, future)
        .await
        .map_err(|_elapsed| IoError::Timeout)?;

    match item {
        Some(Ok(value)) => Ok(value),
        Some(Err(err)) => Err(IoError::Error(err)),
        None => Err(IoError::UnexpectedEof),
    }
}

async fn negotiate_handshake<P, REv>(
    context: &NetworkContext<REv>,
    transport: &mut FramedTransport<P>,
    connection_id: ConnectionId,
) -> Result<(SocketAddr, Option<PublicKey>), ConnectionError>
where
    P: Payload,
{
    // Send down a handshake and expect one in response.
    let handshake = context.chain_info.create_handshake(
        context.public_addr,
        context.consensus_keys.as_ref(),
        connection_id,
    );

    io_timeout(
        context.handshake_timeout.into(),
        transport.send(Arc::new(handshake)),
    )
    .await
    .map_err(ConnectionError::HandshakeSend)?;

    let remote_handshake = io_opt_timeout(context.handshake_timeout.into(), transport.next())
        .await
        .map_err(ConnectionError::HandshakeRecv)?;

    if let Message::Handshake {
        network_name,
        public_addr,
        protocol_version,
        consensus_certificate,
    } = remote_handshake
    {
        debug!(%protocol_version, "handshake received");

        // The handshake was valid, we can check the network name.
        if network_name != context.chain_info.network_name {
            return Err(ConnectionError::WrongNetwork(network_name));
        }

        // If there is a version mismatch, we treat it as a connection error. We do not ban peers
        // for this error, but instead rely on exponential backoff, as bans would result in issues
        // during upgrades where nodes may have a legitimate reason for differing versions.
        //
        // Since we are not using SemVer for versioning, we cannot make any assumptions about
        // compatibility, so we allow only exact version matches.
        if context.reject_incompatible_versions
            && protocol_version != context.chain_info.protocol_version
        {
            if let Some(threshold) = context.tarpit_version_threshold {
                if protocol_version <= threshold {
                    let mut rng = crate::new_rng();

                    if rng.gen_bool(context.tarpit_chance as f64) {
                        // If tarpitting is enabled, we hold open the connection for a specific
                        // amount of time, to reduce load on other nodes and keep them from
                        // reconnecting.
                        info!(duration=?context.tarpit_duration, "randomly tarpitting node");
                        tokio::time::sleep(Duration::from(context.tarpit_duration)).await;
                    } else {
                        debug!(p = context.tarpit_chance, "randomly not tarpitting node");
                    }
                }
            }
            return Err(ConnectionError::IncompatibleVersion(protocol_version));
        }

        let peer_consensus_public_key = consensus_certificate
            .map(|cert| {
                cert.validate(connection_id)
                    .map_err(ConnectionError::InvalidConsensusCertificate)
            })
            .transpose()?;

        Ok((public_addr, peer_consensus_public_key))
    } else {
        // Received a non-handshake, this is an error.
        Err(ConnectionError::DidNotSendHandshake)
    }
}

/// Runs the server core acceptor loop.
pub(super) async fn server<P, REv>(
    context: Arc<NetworkContext<REv>>,
    listener: tokio::net::TcpListener,
    mut shutdown_receiver: watch::Receiver<()>,
) where
    REv: From<Event<P>> + Send,
    P: Payload,
{
    // The server task is a bit tricky, since it has to wait on incoming connections while at the
    // same time shut down if the networking component is dropped, otherwise the TCP socket will
    // stay open, preventing reuse.

    // We first create a future that never terminates, handling incoming connections:
    let accept_connections = async {
        loop {
            // We handle accept errors here, since they can be caused by a temporary resource
            // shortage or the remote side closing the connection while it is waiting in
            // the queue.
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    // The span setup here is used throughout the entire lifetime of the connection.
                    let span =
                        error_span!("incoming", %peer_addr, peer_id=Empty, validator_id=Empty);

                    let context = context.clone();
                    let handler_span = span.clone();
                    tokio::spawn(
                        async move {
                            let incoming =
                                handle_incoming(context.clone(), stream, peer_addr).await;
                            context
                                .event_queue
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

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match future::select(Box::pin(shutdown_messages), Box::pin(accept_connections)).await {
        Either::Left(_) => info!(
            %context.our_id,
            "shutting down socket, no longer accepting incoming connections"
        ),
        Either::Right(_) => unreachable!(),
    }
}

/// Network message reader.
///
/// Schedules all received messages until the stream is closed or an error occurs.
pub(super) async fn message_reader<REv, P>(
    context: Arc<NetworkContext<REv>>,
    mut stream: SplitStream<FramedTransport<P>>,
    limiter: Box<dyn LimiterHandle>,
    mut shutdown_receiver: watch::Receiver<()>,
    peer_id: NodeId,
    span: Span,
) -> io::Result<()>
where
    P: DeserializeOwned + Send + Display + Payload,
    REv: From<Event<P>>,
{
    let read_messages = async move {
        while let Some(msg_result) = stream.next().await {
            match msg_result {
                Ok(msg) => {
                    trace!(%msg, "message received");
                    // We've received a message. Ensure we have the proper amount of resources,
                    // then push it to the reactor.

                    limiter
                        .request_allowance(
                            msg.payload_incoming_resource_estimate(&context.payload_weights),
                        )
                        .await;

                    let queue_kind = if msg.is_low_priority() {
                        QueueKind::NetworkLowPriority
                    } else {
                        QueueKind::NetworkIncoming
                    };

                    context
                        .event_queue
                        .schedule(
                            Event::IncomingMessage {
                                peer_id: Box::new(peer_id),
                                msg: Box::new(msg),
                                span: span.clone(),
                            },
                            queue_kind,
                        )
                        .await;
                }
                Err(err) => {
                    warn!(
                        err = display_error(&err),
                        "receiving message failed, closing connection"
                    );
                    return Err(err);
                }
            }
        }
        Ok(())
    };

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // while loop to terminate.
    match future::select(Box::pin(shutdown_messages), Box::pin(read_messages)).await {
        Either::Left(_) => info!("shutting down incoming connection message reader"),
        Either::Right(_) => (),
    }

    Ok(())
}

/// Network message sender.
///
/// Reads from a channel and sends all messages, until the stream is closed or an error occurs.
pub(super) async fn message_sender<P>(
    mut queue: UnboundedReceiver<Arc<Message<P>>>,
    mut sink: SplitSink<FramedTransport<P>, Arc<Message<P>>>,
    limiter: Box<dyn LimiterHandle>,
    counter: IntGauge,
) where
    P: Payload,
{
    while let Some(message) = queue.recv().await {
        counter.dec();

        // TODO: Refactor message sending to not use `tokio_serde` anymore to avoid duplicate
        //       serialization.
        let estimated_wire_size = rmp_serde::to_vec(&message)
            .as_ref()
            .map(Vec::len)
            .unwrap_or(0) as u32;
        limiter.request_allowance(estimated_wire_size).await;

        // We simply error-out if the sink fails, it means that our connection broke.
        if let Err(ref err) = sink.send(message).await {
            info!(
                err = display_error(err),
                "message send failed, closing outgoing connection"
            );
            break;
        };
    }
}
