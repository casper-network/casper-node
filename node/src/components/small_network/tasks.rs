//! Tasks run by the component.

use std::{
    convert::TryFrom,
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
    Future, SinkExt, StreamExt,
};
use openssl::{
    pkey::{PKey, Private},
    ssl::Ssl,
};
use prometheus::IntGauge;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
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
    bandwidth_limiter::BandwidthLimiterHandle,
    chain_info::ChainInfo,
    counting_format::{ConnectionId, Role},
    error::{display_error, ConnectionError, IoError},
    event::{IncomingConnection, OutgoingConnection},
    message::ConsensusKeyPair,
    BoxedTransportSink, BoxedTransportStream, Event, Message, Payload, Transport, WireProtocol,
};
use crate::{
    components::networking_metrics::NetworkingMetrics,
    reactor::{EventQueueHandle, QueueKind},
    tls::{self, TlsCert},
    types::NodeId,
};

// TODO: Some constants need to be made configurable.

/// Maximum time allowed to send or receive a handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// Maximum size allowed for handshake.
///
/// Set to 128 KB which should be plenty. As the buffer for the handshake is pre-allocated, this
/// constant should not be too large to avoid issues with resource exhaustion attacks.
const HANDSHAKE_MAX_SIZE: u32 = 128 * 1024;

/// Low-level TLS connection function.
///
/// Performs the actual TCP+TLS connection setup.
async fn tls_connect(
    our_cert: &TlsCert,
    secret_key: &PKey<Private>,
    peer_addr: SocketAddr,
) -> Result<(NodeId, Transport), ConnectionError> {
    let stream = TcpStream::connect(peer_addr)
        .await
        .map_err(ConnectionError::TcpConnection)?;

    let mut transport = tls::create_tls_connector(&our_cert.as_x509(), &secret_key)
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
    let (peer_id, mut transport) =
        match tls_connect(&context.our_cert, &context.secret_key, peer_addr).await {
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

    // Negotiate the handshake, concluding the incoming connection process.
    match negotiate_handshake(context.handshake_parameters(connection_id), &mut transport).await {
        Ok((public_addr, peer_consensus_public_key, their_version)) => {
            if let Some(ref public_key) = peer_consensus_public_key {
                Span::current().record("validator_id", &field::display(public_key));
            }

            if public_addr != peer_addr {
                // We don't need the `public_addr`, as we already connected, but warn anyway.
                warn!(%public_addr, %peer_addr, "peer advertises a different public address than what we connected to");
            }

            // Close the receiving end of the transport.
            let (sink, _stream) = WireProtocol::from_protocol_versions::<P>(
                context.chain_info.protocol_version,
                their_version,
                context.net_metrics.clone(),
                connection_id,
                transport,
                Role::Dialer,
                context.chain_info.maximum_net_message_size,
            );

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
    pub(super) net_metrics: Weak<NetworkingMetrics>,
    /// Chain info extract from chainspec.
    pub(super) chain_info: ChainInfo,
    /// Our own public listening address.
    pub(super) public_addr: SocketAddr,
    /// Optional set of consensus keys, to identify as a validator during handshake.
    pub(super) consensus_keys: Option<ConsensusKeyPair>,
}

impl<REv> NetworkContext<REv>
where
    REv: 'static,
{
    /// Generates a set of handshake parameters from the context and given connection ID.
    fn handshake_parameters(&self, connection_id: ConnectionId) -> HandshakeParameters<'_> {
        HandshakeParameters {
            chain_info: &self.chain_info,
            public_addr: self.public_addr,
            consensus_keys: self.consensus_keys.as_ref(),
            connection_id,
        }
    }
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
    let (peer_id, mut transport) =
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

    // Negotiate the handshake, concluding the incoming connection process.
    match negotiate_handshake(context.handshake_parameters(connection_id), &mut transport).await {
        Ok((public_addr, peer_consensus_public_key, their_version)) => {
            if let Some(ref public_key) = peer_consensus_public_key {
                Span::current().record("validator_id", &field::display(public_key));
            }

            // Close the receiving end of the transport.
            let (_sink, stream) = WireProtocol::from_protocol_versions::<P>(
                context.chain_info.protocol_version,
                their_version,
                context.net_metrics.clone(),
                connection_id,
                transport,
                Role::Listener,
                context.chain_info.maximum_net_message_size,
            );

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
    let mut tls_stream = tls::create_tls_acceptor(&cert.as_x509().as_ref(), &secret_key.as_ref())
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

/// Parameters required to negotiate a handshake.
#[derive(Clone, Debug)]
struct HandshakeParameters<'a> {
    /// Chain spec information.
    chain_info: &'a ChainInfo,
    /// Listening address of our node.
    public_addr: SocketAddr,
    /// Optional consensus/validator keys.
    consensus_keys: Option<&'a ConsensusKeyPair>,
    /// The connection ID.
    connection_id: ConnectionId,
}

/// Negotiates handshake.
async fn negotiate_handshake(
    parameters: HandshakeParameters<'_>,
    transport: &mut Transport,
) -> Result<(SocketAddr, Option<PublicKey>, ProtocolVersion), ConnectionError> {
    // Serialize and send a handshake.

    // Note: For legacy reasons, a handshake is encoded as a `Message` enum variant instead of its
    // own type, which requires a payload type to be specified. We use `()` here, as we never expect
    // to send or receive an actual payload.
    let handshake = parameters.chain_info.create_handshake::<()>(
        parameters.public_addr,
        parameters.consensus_keys,
        parameters.connection_id,
    );

    let handshake_serialized =
        rmp_serde::to_vec(&handshake).map_err(ConnectionError::HandshakeSerialization)?;

    io_timeout(HANDSHAKE_TIMEOUT, async {
        let len = u32::try_from(handshake_serialized.len()).expect("handshake is too large?");
        assert!(len < HANDSHAKE_MAX_SIZE);
        transport.write_all(&len.to_be_bytes()).await?;
        transport.write_all(&handshake_serialized).await
    })
    .await
    .map_err(ConnectionError::HandshakeSend)?;

    // Receive remote handshake and decode it.
    let remote_handshake_raw = io_timeout(HANDSHAKE_TIMEOUT, async {
        let mut len_raw = [0u8; 4];
        transport.read_exact(&mut len_raw).await?;

        let len = u32::from_be_bytes(len_raw);
        if len > HANDSHAKE_MAX_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "advertised remote handshake size of {} exceeds limit of {}",
                    len, HANDSHAKE_MAX_SIZE
                ),
            ));
        }

        let mut handshake_raw = Vec::new();
        transport
            .take(u64::from(len))
            .read_to_end(&mut handshake_raw)
            .await?;

        Ok(handshake_raw)
    })
    .await
    .map_err(ConnectionError::HandshakeRecv)?;

    let remote_handshake: Message<()> = rmp_serde::from_read(io::Cursor::new(remote_handshake_raw))
        .map_err(ConnectionError::HandshakeDeserialization)?;

    // Now we can process it.
    if let Message::Handshake {
        network_name,
        public_addr,
        protocol_version,
        consensus_certificate,
    } = remote_handshake
    {
        debug!(%protocol_version, "handshake received");

        // The handshake was valid, we can check the network name.
        if network_name != parameters.chain_info.network_name {
            return Err(ConnectionError::WrongNetwork(network_name));
        }

        let peer_consensus_public_key = consensus_certificate
            .map(|cert| {
                cert.validate(parameters.connection_id)
                    .map_err(ConnectionError::InvalidConsensusCertificate)
            })
            .transpose()?;

        Ok((public_addr, peer_consensus_public_key, protocol_version))
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
    mut stream: BoxedTransportStream<P>,
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
                    // We've received a message, push it to the reactor.
                    context
                        .event_queue
                        .schedule(
                            Event::IncomingMessage {
                                peer_id: Box::new(peer_id),
                                msg: Box::new(msg),
                                span: span.clone(),
                            },
                            QueueKind::NetworkIncoming,
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
    mut sink: BoxedTransportSink<P>,
    limiter: Box<dyn BandwidthLimiterHandle>,
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

#[cfg(test)]
mod tests {
    use std::{error::Error as StdError, net::SocketAddr, sync::Arc, time::Duration};

    use casper_types::PublicKey;
    use futures::{Future, SinkExt, StreamExt};
    use prometheus::Registry;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tracing::debug;

    use crate::{
        components::{
            networking_metrics::NetworkingMetrics,
            small_network::{
                chain_info::ChainInfo,
                counting_format::{ConnectionId, Role},
                error::{ConnectionError, IoError},
                FramedTransport, Payload, SmallNetworkIdentity, Transport, WireProtocol,
            },
        },
        protocol::Message,
        testing::init_logging,
        types::NodeId,
    };

    use super::{
        io_timeout, negotiate_handshake, server_setup_tls, tls_connect, HandshakeParameters,
        HANDSHAKE_TIMEOUT,
    };

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

    /// Negotiates a connection handshake over the given transport.
    ///
    /// This function uses the wire protocol to frame handshake messages and thus emulates the V1
    /// behavior of the node.
    #[cfg(test)]
    async fn negotiate_handshake_using_wire_protocol<P>(
        parameters: HandshakeParameters<'_>,
        transport: &mut FramedTransport<P>,
    ) -> Result<(SocketAddr, Option<PublicKey>), ConnectionError>
    where
        P: Payload,
    {
        // Send down a handshake and expect one in response.
        let handshake = parameters.chain_info.create_handshake(
            parameters.public_addr,
            parameters.consensus_keys,
            parameters.connection_id,
        );

        io_timeout(HANDSHAKE_TIMEOUT, transport.send(Arc::new(handshake)))
            .await
            .map_err(ConnectionError::HandshakeSend)?;

        let remote_handshake = io_opt_timeout(HANDSHAKE_TIMEOUT, transport.next())
            .await
            .map_err(ConnectionError::HandshakeRecv)?;

        if let crate::components::small_network::Message::Handshake {
            network_name,
            public_addr,
            protocol_version,
            consensus_certificate,
        } = remote_handshake
        {
            debug!(%protocol_version, "handshake received");

            // The handshake was valid, we can check the network name.
            if network_name != parameters.chain_info.network_name {
                return Err(ConnectionError::WrongNetwork(network_name));
            }

            let peer_consensus_public_key = consensus_certificate
                .map(|cert| {
                    cert.validate(parameters.connection_id)
                        .map_err(ConnectionError::InvalidConsensusCertificate)
                })
                .transpose()?;

            Ok((public_addr, peer_consensus_public_key))
        } else {
            // Received a non-handshake, this is an error.
            Err(ConnectionError::DidNotSendHandshake)
        }
    }
    /// An established TLS connection pair.
    struct TlsConnectionPair {
        /// Server end of the connection.
        server_transport: Transport,
        /// Client end of the connection.
        client_transport: Transport,
        /// The connection ID on both ends.
        connection_id: ConnectionId,
    }

    /// Creates a established TLS connection pair using the same TLS setup that the node itself
    /// would use.
    ///
    /// Also initializes logging.
    async fn create_tls_connection_pair() -> TlsConnectionPair {
        // We always want logging with TLS connection pair tests.
        init_logging();

        // Setup TLS keys.
        let server_identity =
            SmallNetworkIdentity::new().expect("could not generate server identity");
        let server_id = NodeId::from(&server_identity);
        let client_identity =
            SmallNetworkIdentity::new().expect("could not generate client identity");
        let client_id = NodeId::from(&client_identity);

        // Setup the TCP/IP listener.
        let tcp_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("could not bind testing tcp listener");
        let listen_addr = tcp_listener
            .local_addr()
            .expect("could not get listening address");

        // As as separate task, begin the connection process (the port is already bound).
        let connect_handle = tokio::spawn(async move {
            tls_connect(
                &client_identity.tls_certificate,
                &client_identity.secret_key,
                listen_addr,
            )
            .await
        });

        let (stream, _peer_addr) = tcp_listener
            .accept()
            .await
            .expect("could not accept incoming connection");

        let (peer_id_from_server_pov, server_transport) = server_setup_tls(
            stream,
            &server_identity.tls_certificate,
            &server_identity.secret_key,
        )
        .await
        .expect("could not setup TLS on server side");

        assert_eq!(peer_id_from_server_pov, client_id);

        // Join the client task.
        let (peer_id_from_client_pov, client_transport) = connect_handle
            .await
            .expect("could not join client task")
            .expect("connection failed");

        assert_eq!(peer_id_from_client_pov, server_id);

        let connection_id_server =
            ConnectionId::from_connection(server_transport.ssl(), server_id, client_id);
        let connection_id_client =
            ConnectionId::from_connection(client_transport.ssl(), client_id, server_id);

        // Technically, these should always be the same, we are including an extra check here.
        assert_eq!(connection_id_server, connection_id_client);

        TlsConnectionPair {
            server_transport,
            client_transport,
            connection_id: connection_id_server,
        }
    }

    #[tokio::test]
    async fn can_establish_working_tls_connection() {
        let mut connection_pair = create_tls_connection_pair().await;

        const SHORT_MESSAGE: [u8; 8] = [0, 1, 2, 3, 99, 100, 101, 102];

        // Note: We're counting on buffering making the send essentially non-blocking here. It is
        // important to send very small amount, one that is guaranteed to be immediately accepted
        // and buffered by the kernel.

        connection_pair
            .server_transport
            .write_all(&SHORT_MESSAGE)
            .await
            .expect("write failed");

        let mut recv_buffer = [0u8; 8];

        connection_pair
            .client_transport
            .read(&mut recv_buffer)
            .await
            .expect("read failed");

        assert_eq!(recv_buffer, SHORT_MESSAGE);
    }

    #[tokio::test]
    async fn can_handshake_with_itself_using_legacy_handshake() {
        let TlsConnectionPair {
            server_transport,
            client_transport,
            connection_id,
        } = create_tls_connection_pair().await;

        let server_public_addr = server_transport
            .get_ref()
            .local_addr()
            .expect("failed to get server_addr");

        let registry = Registry::new();
        let metrics = Arc::new(NetworkingMetrics::new(&registry).expect("could not setup metrics"));

        let mut server_framed = WireProtocol::V1.framed::<Message>(
            Arc::downgrade(&metrics),
            connection_id,
            server_transport,
            Role::Listener,
            10_000_000,
        );

        let mut client_framed = WireProtocol::V1.framed::<Message>(
            Arc::downgrade(&metrics),
            connection_id,
            client_transport,
            Role::Listener,
            10_000_000,
        );

        // RFC5737 address, as we are not running a client to connect back to.
        let client_public_addr = "192.0.2.1:12345".parse().unwrap();

        // Start the clients handshake negotation.
        let client_handshake = tokio::spawn(async move {
            let chain_info = ChainInfo::create_for_testing();
            let handshake_parameters = HandshakeParameters {
                chain_info: &chain_info,
                public_addr: client_public_addr,
                consensus_keys: None,
                connection_id,
            };

            negotiate_handshake_using_wire_protocol(handshake_parameters, &mut client_framed).await
        });

        // Perform the same on the server side.
        let chain_info = ChainInfo::create_for_testing();
        let handshake_parameters = HandshakeParameters {
            chain_info: &chain_info,
            public_addr: server_public_addr,
            consensus_keys: None,
            connection_id,
        };
        let (server_received_public_addr, server_received_public_key) =
            negotiate_handshake_using_wire_protocol(handshake_parameters, &mut server_framed)
                .await
                .expect("server handshake failed");
        assert_eq!(server_received_public_addr, client_public_addr);
        assert!(server_received_public_key.is_none());

        let (client_received_public_addr, client_received_public_key) = client_handshake
            .await
            .expect("client handshake not joined")
            .expect("client handshake failed");
        assert_eq!(client_received_public_addr, server_public_addr);
        assert!(client_received_public_key.is_none());
    }

    #[tokio::test]
    async fn can_handshake_across_handshake_implementations() {
        let TlsConnectionPair {
            mut server_transport,
            client_transport,
            connection_id,
        } = create_tls_connection_pair().await;

        let server_public_addr = server_transport
            .get_ref()
            .local_addr()
            .expect("failed to get server_addr");

        let registry = Registry::new();
        let metrics = Arc::new(NetworkingMetrics::new(&registry).expect("could not setup metrics"));

        let mut client_framed = WireProtocol::V1.framed::<Message>(
            Arc::downgrade(&metrics),
            connection_id,
            client_transport,
            Role::Listener,
            10_000_000,
        );

        // RFC5737 address, as we are not running a client to connect back to.
        let client_public_addr = "192.0.2.1:12345".parse().unwrap();

        // Start the clients handshake negotation.
        let client_handshake = tokio::spawn(async move {
            let chain_info = ChainInfo::create_for_testing();
            let handshake_parameters = HandshakeParameters {
                chain_info: &chain_info,
                public_addr: client_public_addr,
                consensus_keys: None,
                connection_id,
            };

            negotiate_handshake_using_wire_protocol(handshake_parameters, &mut client_framed).await
        });

        // Perform the same on the server side.
        let chain_info = ChainInfo::create_for_testing();
        let handshake_parameters = HandshakeParameters {
            chain_info: &chain_info,
            public_addr: server_public_addr,
            consensus_keys: None,
            connection_id,
        };
        let (server_received_public_addr, server_received_public_key, _their_version) =
            negotiate_handshake(handshake_parameters, &mut server_transport)
                .await
                .expect("server handshake failed");
        assert_eq!(server_received_public_addr, client_public_addr);
        assert!(server_received_public_key.is_none());

        let (client_received_public_addr, client_received_public_key) = client_handshake
            .await
            .expect("client handshake not joined")
            .expect("client handshake failed");
        assert_eq!(client_received_public_addr, server_public_addr);
        assert!(client_received_public_key.is_none());
    }
}
