//! Fully connected overlay network
//!
//! The *small network* is an overlay network where each node participating is connected to every
//! other node on the network. The *small* portion of the name stems from the fact that this
//! approach is not scalable, as it requires at least $O(n)$ network connections and broadcast will
//! result in $O(n^2)$ messages.
//!
//! # Node IDs
//!
//! Each node has a self-generated node ID based on its self-signed TLS certificate. Whenever a
//! connection is made to another node, it verifies the "server"'s certificate to check that it
//! connected to the correct node and sends its own certificate during the TLS handshake,
//! establishing identity.
//!
//! # Messages and payloads
//!
//! The network itself is best-effort, during regular operation, no messages should be lost. A node
//! will attempt to reconnect when it loses a connection, however messages and broadcasts may be
//! lost during that time.
//!
//! # Connection
//!
//! Every node has an ID and a listening address. The objective of each node is to constantly
//! maintain an outgoing connection to each other node (and thus have an incoming connection from
//! these nodes as well).
//!
//! Any incoming connection is strictly read from, while any outgoing connection is strictly used
//! for sending messages.
//!
//! Nodes track the signed (timestamp, listening address, certificate) tuples called "endpoints"
//! internally and whenever they connecting to a new node, they share this state with the other
//! node, as well as notifying them about any updates they receive.
//!
//! # Joining the network
//!
//! When a node connects to any other network node, it sends its current list of endpoints down the
//! new outgoing connection. This will cause the receiving node to initiate a connection attempt to
//! all nodes in the list and simultaneously tell all of its connected nodes about the new node,
//! repeating the process.

mod config;
mod endpoint;
mod event;
mod message;

use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    io,
    net::{SocketAddr, TcpListener},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Context};
use futures::{
    stream::{SplitSink, SplitStream},
    FutureExt, SinkExt, StreamExt,
};
use maplit::hashmap;
use openssl::{pkey, x509::X509};
use pkey::{PKey, Private};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    net::TcpStream,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};
use tokio_openssl::SslStream;
use tokio_serde::{formats::SymmetricalMessagePack, SymmetricallyFramed};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info, warn};

pub(crate) use self::{endpoint::Endpoint, event::Event, message::Message};
use crate::{
    components::Component,
    effect::{requests::NetworkRequest, Effect, EffectExt, EffectResultExt, Multiple},
    reactor::{EventQueueHandle, QueueKind, Reactor},
    tls::{self, KeyFingerprint, Signed, TlsCert},
};
pub use config::Config;

/// A node ID.
///
/// The key fingerprint found on TLS certificates.
type NodeId = KeyFingerprint;

pub(crate) struct SmallNetwork<R, P>
where
    R: Reactor,
{
    /// Configuration.
    cfg: Config,
    /// Server certificate.
    cert: Arc<X509>,
    /// Server private key.
    private_key: Arc<PKey<Private>>,
    /// Handle to event queue.
    eq: EventQueueHandle<R, Event<P>>,
    /// A list of known endpoints by node ID.
    endpoints: HashMap<NodeId, Endpoint>,
    /// Stored signed endpoints that can be sent to other nodes.
    signed_endpoints: HashMap<NodeId, Signed<Endpoint>>,
    /// Outgoing network connections' messages.
    outgoing: HashMap<NodeId, UnboundedSender<Message<P>>>,
}

impl<R, P> SmallNetwork<R, P>
where
    R: Reactor + 'static,
    P: Serialize + DeserializeOwned + Clone + Debug + Send + 'static,
{
    #[allow(clippy::type_complexity)]
    pub(crate) fn new(
        eq: EventQueueHandle<R, Event<P>>,
        cfg: Config,
    ) -> anyhow::Result<(SmallNetwork<R, P>, Multiple<Effect<Event<P>>>)>
    where
        R: Reactor + 'static,
    {
        // First, we load or generate the TLS keys.
        let (cert, private_key) = match (&cfg.cert, &cfg.private_key) {
            // We're given a cert_file and a private_key file. Just load them, additional checking
            // will be performed once we create the acceptor and connector.
            (Some(cert_file), Some(private_key_file)) => (
                tls::load_cert(cert_file).context("could not load TLS certificate")?,
                tls::load_private_key(private_key_file)
                    .context("could not load TLS private key")?,
            ),

            // Neither was passed, so we auto-generate a pair.
            (None, None) => tls::generate_node_cert()?,

            // If we get only one of the two, return an error.
            _ => bail!("need either both or none of cert, private_key in network config"),
        };

        // We can now create a listener.
        let listener = create_listener(&cfg)?;
        let addr = listener.local_addr()?;

        // Create the model. Initially we know our own endpoint address.
        let our_endpoint = Endpoint::new(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64,
            addr,
            tls::validate_cert(cert.clone())?,
        );
        let our_fp = our_endpoint.cert().public_key_fingerprint();

        // Run the server task.
        info!(%our_endpoint, "starting server background task");
        let mut effects = server_task(eq, tokio::net::TcpListener::from_std(listener)?)
            .boxed()
            .ignore();
        let model = SmallNetwork {
            cfg,
            signed_endpoints: hashmap! { our_fp => Signed::new(&our_endpoint, &private_key)? },
            endpoints: hashmap! { our_fp => our_endpoint },
            cert: Arc::new(cert),
            private_key: Arc::new(private_key),
            eq,
            outgoing: HashMap::new(),
        };

        // Connect to the root node (even if we are the root node, just loopback).
        effects.extend(model.connect_to_root());

        Ok((model, effects))
    }

    /// Attempts to connect to the root node.
    fn connect_to_root(&self) -> Multiple<Effect<Event<P>>> {
        connect_trusted(
            self.cfg.root_addr,
            self.cert.clone(),
            self.private_key.clone(),
        )
        .result(
            move |(cert, transport)| Event::RootConnected { cert, transport },
            move |error| Event::RootFailed { error },
        )
    }

    /// Queues a message to be sent to all nodes.
    fn broadcast_message(&self, msg: Message<P>) {
        for node_id in self.outgoing.keys() {
            self.send_message(*node_id, msg.clone());
        }
    }

    /// Queues a message to be sent to a specific node.
    fn send_message(&self, dest: NodeId, msg: Message<P>) {
        // Try to send the message.
        if let Some(sender) = self.outgoing.get(&dest) {
            if let Err(msg) = sender.send(msg) {
                // We lost the connection, but that fact has not reached us yet.
                warn!(%dest, ?msg, "dropped outgoing message, lost connection");
            }
        } else {
            // We are not connected, so the reconnection is likely already in progress.
            warn!(%dest, ?msg, "dropped outgoing message, no connection");
        }
    }

    /// Updates the internal endpoint store from a given endpoint.
    ///
    /// Returns the node ID of the endpoint if it was new.
    #[inline]
    fn update_endpoint(&mut self, endpoint: &Endpoint) -> Option<NodeId> {
        let fp = endpoint.cert().public_key_fingerprint();

        if let Some(prev) = self.endpoints.get(&fp) {
            if prev >= endpoint {
                // Still up to date or stale, do nothing.
                return None;
            }
        }

        self.endpoints.insert(fp, endpoint.clone());
        Some(fp)
    }

    /// Updates internal endpoint store and if new, output a `BroadcastEndpoint` effect.
    #[inline]
    fn update_and_broadcast_if_new(
        &mut self,
        signed: Signed<Endpoint>,
    ) -> Multiple<Effect<Event<P>>> {
        match signed.validate_self_signed(|endpoint| Ok(endpoint.cert().public_key())) {
            Ok(endpoint) => {
                // Endpoint is valid, check if it was new.
                if let Some(node_id) = self.update_endpoint(&endpoint) {
                    debug!("new endpoint {}", endpoint);
                    // We learned of a new endpoint. We store it and note whether it is the first
                    // endpoint for the node.
                    self.signed_endpoints.insert(node_id, signed.clone());
                    self.endpoints.insert(node_id, endpoint.clone());

                    let effect = if self.outgoing.remove(&node_id).is_none() {
                        info!(%node_id, %endpoint, "new outgoing channel");
                        // Initiate the connection process once we learn of a new node ID.
                        connect_outgoing(endpoint, self.cert.clone(), self.private_key.clone())
                            .result(
                                move |transport| Event::OutgoingEstablished { node_id, transport },
                                move |error| Event::OutgoingFailed {
                                    node_id,
                                    attempt_count: 0,
                                    error: Some(error),
                                },
                            )
                    } else {
                        // There was a previous endpoint, whose sender has now been dropped. This
                        // will cause the sender task to exit and trigger a reconnect.

                        info!(%endpoint, "endpoint changed");
                        Multiple::new()
                    };

                    self.broadcast_message(Message::BroadcastEndpoint(signed));

                    effect
                } else {
                    debug!("known endpoint: {}", endpoint);
                    Multiple::new()
                }
            }
            Err(err) => {
                warn!(%err, ?signed, "received invalid endpoint");
                Multiple::new()
            }
        }
    }

    /// Sets up an established outgoing connection.
    fn setup_outgoing(
        &mut self,
        node_id: NodeId,
        transport: Transport,
    ) -> Multiple<Effect<Event<P>>> {
        // This connection is send-only, we only use the sink.
        let (sink, _stream) = framed::<P>(transport).split();

        let (sender, receiver) = mpsc::unbounded_channel();
        if self.outgoing.insert(node_id, sender).is_some() {
            // We assume that for a reconnect to have happened, the outgoing entry must have
            // been either non-existent yet or cleaned up by the handler of the connection
            // closing event. If this is not the case, an assumed invariant has been violated.
            error!(%node_id, "did not expect leftover channel in outgoing map");
        }

        // We can now send a snapshot.
        let snapshot = Message::Snapshot(self.signed_endpoints.values().cloned().collect());
        self.send_message(node_id, snapshot);

        message_sender(receiver, sink).event(move |result| Event::OutgoingFailed {
            node_id,
            attempt_count: 0, // reset to 0, since we have had a successful connection
            error: result.err().map(Into::into),
        })
    }

    /// Handles a received message.
    // Internal function to keep indentation and nesting sane.
    fn handle_message(&mut self, node_id: NodeId, msg: Message<P>) -> Multiple<Effect<Event<P>>> {
        match msg {
            Message::Snapshot(snapshot) => snapshot
                .into_iter()
                .map(|signed| self.update_and_broadcast_if_new(signed))
                .flatten()
                .collect(),
            Message::BroadcastEndpoint(signed) => self.update_and_broadcast_if_new(signed),
            Message::Payload(payload) => {
                // We received a message payload.
                warn!(
                    %node_id,
                    ?payload,
                    "received message payload, but no implementation for what comes next"
                );
                Multiple::new()
            }
        }
    }
}

impl<R, P> Component for SmallNetwork<R, P>
where
    R: Reactor + 'static,
    P: Serialize + DeserializeOwned + Clone + Debug + Send + 'static,
{
    type Event = Event<P>;

    #[allow(clippy::cognitive_complexity)]
    fn handle_event(&mut self, ev: Self::Event) -> Multiple<Effect<Self::Event>> {
        match ev {
            Event::RootConnected { cert, transport } => {
                // Create a pseudo-endpoint for the root node with the lowest priority (time 0)
                let root_node_id = cert.public_key_fingerprint();
                let ep = Endpoint::new(0, self.cfg.root_addr, cert);
                if self.endpoints.insert(root_node_id, ep).is_some() {
                    // This connection is the very first we will ever make, there should never be
                    // a root node registered, as we will never re-attempt this connection if it
                    // succeeded once.
                    error!("Encountered a second root node connection.")
                }

                // We're now almost setup exactly as if the root node was any other node, proceed
                // as normal.
                self.setup_outgoing(root_node_id, transport)
            }
            Event::RootFailed { error } => {
                warn!(%error, "connection to root failed");
                self.connect_to_root()

                // TODO: delay next attempt
            }
            Event::IncomingNew { stream, addr } => {
                debug!(%addr, "Incoming connection, starting TLS handshake");

                setup_tls(stream, self.cert.clone(), self.private_key.clone())
                    .boxed()
                    .event(move |result| Event::IncomingHandshakeCompleted { result, addr })
            }
            Event::IncomingHandshakeCompleted { result, addr } => {
                match result {
                    Ok((fp, transport)) => {
                        // The sink is never used, as we only read data from incoming connections.
                        let (_sink, stream) = framed::<P>(transport).split();

                        message_reader(self.eq, stream, fp)
                            .event(move |result| Event::IncomingClosed { result, addr })
                    }
                    Err(err) => {
                        warn!(%addr, %err, "TLS handshake failed");
                        Multiple::new()
                    }
                }
            }
            Event::IncomingMessage { node_id, msg } => self.handle_message(node_id, msg),
            Event::IncomingClosed { result, addr } => {
                match result {
                    Ok(()) => info!(%addr, "connection closed"),
                    Err(err) => warn!(%addr, %err, "connection dropped"),
                }
                Multiple::new()
            }
            Event::OutgoingEstablished { node_id, transport } => {
                self.setup_outgoing(node_id, transport)
            }
            Event::OutgoingFailed {
                node_id,
                attempt_count,
                error,
            } => {
                if let Some(err) = error {
                    warn!(%node_id, %err, "outgoing connection failed");
                } else {
                    warn!(%node_id, "outgoing connection closed");
                }

                if let Some(max) = self.cfg.max_outgoing_retries {
                    if attempt_count >= max {
                        // We're giving up connecting to the node. We will remove it completely
                        // (this only carries the danger of the stale addresses being sent to us by
                        // other nodes again).
                        self.endpoints.remove(&node_id);
                        self.signed_endpoints.remove(&node_id);
                        self.outgoing.remove(&node_id);

                        warn!(%attempt_count, %node_id, "giving up on outgoing connection");
                    }

                    return Multiple::new();
                }
                // TODO: Delay reconnection.

                if let Some(endpoint) = self.endpoints.get(&node_id) {
                    connect_outgoing(
                        endpoint.clone(),
                        self.cert.clone(),
                        self.private_key.clone(),
                    )
                    .result(
                        move |transport| Event::OutgoingEstablished { node_id, transport },
                        move |error| Event::OutgoingFailed {
                            node_id,
                            attempt_count: attempt_count + 1,
                            error: Some(error),
                        },
                    )
                } else {
                    error!("endpoint disappeared");
                    Multiple::new()
                }
            }
            Event::NetworkRequest {
                req:
                    NetworkRequest::SendMessage {
                        dest,
                        payload,
                        responder,
                    },
            } => {
                // We're given a message to send out.
                responder
                    .respond(self.send_message(dest, Message::Payload(payload)))
                    .ignore()
            }
            Event::NetworkRequest {
                req: NetworkRequest::BroadcastMessage { payload, responder },
            } => {
                // We're given a message to send out.
                responder
                    .respond(self.broadcast_message(Message::Payload(payload)))
                    .ignore()
            }
        }
    }
}

/// Determines bind address for now.
///
/// Will attempt to bind on the root address first if the `bind_interface` is the same as the
/// interface of `root_addr`. Otherwise uses an unused port on `bind_interface`.
fn create_listener(cfg: &Config) -> io::Result<TcpListener> {
    if cfg.root_addr.ip() == cfg.bind_interface {
        // Try to become the root node, if the root nodes interface is available.
        match TcpListener::bind(cfg.root_addr) {
            Ok(listener) => {
                info!("we are the root node!");
                return Ok(listener);
            }
            Err(err) => {
                warn!(
                    %err,
                    "could not bind to {}, will become a non-root node", cfg.root_addr
                );
            }
        };
    }

    // We did not become the root node, bind on random port.
    Ok(TcpListener::bind((cfg.bind_interface, 0u16))?)
}

/// Core accept loop for the networking server.
///
/// Never terminates.
async fn server_task<P, R: Reactor>(
    eq: EventQueueHandle<R, Event<P>>,
    mut listener: tokio::net::TcpListener,
) {
    loop {
        // We handle accept errors here, since they can be caused by a temporary resource shortage
        // or the remote side closing the connection while it is waiting in the queue.
        match listener.accept().await {
            Ok((stream, addr)) => {
                // Move the incoming connection to the event queue for handling.
                let ev = Event::IncomingNew { stream, addr };
                eq.schedule(ev, QueueKind::NetworkIncoming).await;
            }
            Err(err) => warn!(%err, "dropping incoming connection during accept"),
        }
    }
}

/// Server-side TLS handshake.
///
/// This function groups the TLS handshake into a convenient function, enabling the `?` operator.
async fn setup_tls(
    stream: TcpStream,
    cert: Arc<X509>,
    private_key: Arc<PKey<Private>>,
) -> anyhow::Result<(NodeId, Transport)> {
    let tls_stream = tokio_openssl::accept(
        &tls::create_tls_acceptor(&cert.as_ref(), &private_key.as_ref())?,
        stream,
    )
    .await?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or_else(|| anyhow!("no peer certificate presented"))?;

    Ok((
        tls::validate_cert(peer_cert)?.public_key_fingerprint(),
        tls_stream,
    ))
}

/// Network message reader.
///
/// Schedules all received messages until the stream is closed or an error occurs.
async fn message_reader<R, P>(
    eq: EventQueueHandle<R, Event<P>>,
    mut stream: SplitStream<FramedTransport<P>>,
    node_id: NodeId,
) -> io::Result<()>
where
    R: Reactor,
    P: DeserializeOwned + Send,
{
    while let Some(msg_result) = stream.next().await {
        match msg_result {
            Ok(msg) => {
                // We've received a message, push it to the reactor.
                eq.schedule(
                    Event::IncomingMessage { node_id, msg },
                    QueueKind::NetworkIncoming,
                )
                .await;
            }
            Err(err) => {
                warn!(%err, "receiving message failed, closing connection");
                return Err(err);
            }
        }
    }
    Ok(())
}

/// Network message sender.
///
/// Reads from a channel and sends all messages, until the stream is closed or an error occurs.
async fn message_sender<P>(
    mut queue: UnboundedReceiver<Message<P>>,
    mut sink: SplitSink<FramedTransport<P>, Message<P>>,
) -> io::Result<()>
where
    P: Serialize + Send,
{
    while let Some(payload) = queue.recv().await {
        // We simply error-out if the sink fails, it means that our connection broke.
        sink.send(payload).await?;
    }

    Ok(())
}

/// Transport type alias for base encrypted connections.
type Transport = SslStream<TcpStream>;

/// A framed transport for `Message`s.
type FramedTransport<P> = SymmetricallyFramed<
    Framed<Transport, LengthDelimitedCodec>,
    Message<P>,
    SymmetricalMessagePack<Message<P>>,
>;

/// Constructs a new framed transport on a stream.
fn framed<P>(stream: Transport) -> FramedTransport<P> {
    let length_delimited = Framed::new(stream, LengthDelimitedCodec::new());
    SymmetricallyFramed::new(
        length_delimited,
        SymmetricalMessagePack::<Message<P>>::default(),
    )
}

/// Initiates a TLS connection to an endpoint.
async fn connect_outgoing(
    endpoint: Endpoint,
    cert: Arc<X509>,
    private_key: Arc<PKey<Private>>,
) -> anyhow::Result<Transport> {
    let (server_cert, transport) = connect_trusted(endpoint.addr(), cert, private_key).await?;

    let remote_id = server_cert.public_key_fingerprint();

    if remote_id != endpoint.cert().public_key_fingerprint() {
        bail!("remote node has wrong ID");
    }

    Ok(transport)
}

/// Initiates a TLS connection to a remote address, regardless of what ID the remote node reports.
async fn connect_trusted(
    addr: SocketAddr,
    cert: Arc<X509>,
    private_key: Arc<PKey<Private>>,
) -> anyhow::Result<(TlsCert, Transport)> {
    let mut config = tls::create_tls_connector(&cert, &private_key)
        .context("could not create TLS connector")?
        .configure()?;
    config.set_verify_hostname(false);

    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .context("TCP connection failed")?;

    let tls_stream = tokio_openssl::connect(config, "this-will-not-be-checked.example.com", stream)
        .await
        .context("tls handshake failed")?;

    let server_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or_else(|| anyhow!("no server certificate presented"))?;
    Ok((tls::validate_cert(server_cert)?, tls_stream))
}

impl<R, P> Debug for SmallNetwork<R, P>
where
    R: Reactor,
    P: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmallNetwork")
            .field("cert", &"<SSL cert>")
            .field("private_key", &"<hidden>")
            .field("eq", &"<eq>")
            .field("endpoints", &self.endpoints)
            .field("signed_endpoints", &self.signed_endpoints)
            .field("outgoing", &self.outgoing)
            .finish()
    }
}
