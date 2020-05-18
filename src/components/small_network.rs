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

use crate::effect::{Effect, EffectExt, EffectResultExt};
use crate::tls::{self, Fingerprint, Signed, TlsCert};
use crate::util::Multiple;
use crate::{config, reactor};
use anyhow::Context;
use futures::{FutureExt, SinkExt, StreamExt};
use maplit::hashmap;
use openssl::{pkey, x509};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::hash::Hash;
use std::sync::Arc;
use std::{cmp, collections, fmt, io, net, time};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message<P> {
    /// A pruned set of all endpoint announcements the server has received.
    Snapshot(collections::HashSet<Signed<Endpoint>>),
    /// Broadcast a new endpoint known to the sender.
    BroadcastEndpoint(Signed<Endpoint>),
    /// A payload message.
    Payload(P),
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Endpoint {
    /// UNIX timestamp in nanoseconds resolution.
    timestamp_ns: u128,
    /// Socket address the node is listening on.
    addr: net::SocketAddr,
    /// Certificate.
    cert: TlsCert,
}

#[derive(Debug)]
pub enum Event<P> {
    /// A new TCP connection has been established from an incoming connection.
    IncomingNew {
        stream: tokio::net::TcpStream,
        addr: net::SocketAddr,
    },
    /// The TLS handshake completed on the incoming connection.
    IncomingHandshakeCompleted {
        result: anyhow::Result<(Fingerprint, Transport)>,
        addr: net::SocketAddr,
    },
    /// Received network message.
    IncomingMessage {
        node_id: Fingerprint,
        msg: Message<P>,
    },
    /// Incoming connection closed.
    IncomingClosed {
        result: io::Result<()>,
        addr: net::SocketAddr,
    },

    /// A new outgoing connection was successfully established.
    OutgoingEstablished {
        node_id: Fingerprint,
        transport: Transport,
    },
    /// An outgoing connection failed to connect or was terminated.
    OutgoingFailed {
        node_id: Fingerprint,
        attempt_count: u32,
        error: Option<anyhow::Error>,
    },
}

pub struct SmallNetwork<R, P>
where
    R: reactor::Reactor,
{
    /// Configuration.
    cfg: config::SmallNetwork,
    /// Server certificate.
    cert: Arc<x509::X509>,
    /// Server private key.
    private_key: Arc<pkey::PKey<pkey::Private>>,
    /// Handle to event queue.
    eq: reactor::EventQueueHandle<R, Event<P>>,
    /// A list of known endpoints by node id.
    endpoints: collections::HashMap<Fingerprint, Endpoint>,
    /// Stored signed endpoints that can be sent to other nodes.
    signed_endpoints: collections::HashMap<Fingerprint, Signed<Endpoint>>,
    /// Outgoing network connections messages.
    outgoing: collections::HashMap<Fingerprint, mpsc::UnboundedSender<Message<P>>>,
}

impl<R, P> SmallNetwork<R, P>
where
    R: reactor::Reactor + 'static,
    P: Serialize + DeserializeOwned + Clone + fmt::Debug + Send + 'static,
{
    pub fn new(
        eq: reactor::EventQueueHandle<R, Event<P>>,
        cfg: config::SmallNetwork,
    ) -> anyhow::Result<(SmallNetwork<R, P>, Multiple<Effect<Event<P>>>)>
    where
        R: reactor::Reactor + 'static,
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
            _ => anyhow::bail!("need either both or none of cert, private_key in network config"),
        };

        // We can now create a listener.
        let rt = tokio::runtime::Handle::current();
        let listener = rt.block_on(create_listener(&cfg))?;
        let addr = listener.local_addr()?;

        // Create the model. Initially we know our own endpoint address.
        let our_endpoint = Endpoint {
            timestamp_ns: time::SystemTime::now()
                .duration_since(time::UNIX_EPOCH)?
                .as_nanos(),
            addr,
            cert: TlsCert::new(cert.clone())?,
        };
        let our_fp = our_endpoint.cert.public_key_fingerprint()?;

        let model = SmallNetwork {
            cfg,
            signed_endpoints: hashmap! { our_fp => Signed::new(&our_endpoint, &private_key)? },
            endpoints: hashmap! { our_fp => our_endpoint },
            cert: Arc::new(cert),
            private_key: Arc::new(private_key),
            eq,
            outgoing: collections::HashMap::new(),
        };

        // Run the server task.
        info!(?addr, "starting server background task");
        let effects = server_task(eq, listener).boxed().ignore();

        Ok((model, effects))
    }

    pub fn handle_event(&mut self, ev: Event<P>) -> Multiple<Effect<Event<P>>> {
        match ev {
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
                    Err(err) => warn!(%addr, %err, "connection dopped"),
                }
                Multiple::new()
            }
            Event::OutgoingEstablished { node_id, transport } => {
                // This connection is send-only, we only use the sink.
                let (sink, _stream) = framed::<P>(transport).split();

                let (sender, receiver) = mpsc::unbounded_channel();
                if self.outgoing.insert(node_id, sender).is_some() {
                    // We assume that for a reconnect to have happened, the outgoing entry must have
                    // been either non-existant yet or cleaned up by the handler of the connection
                    // closing event. If this not the case, an assumed invariant has been violated.
                    error!(%node_id, "did not expect leftover channel in outgoing map");
                }

                // We can now send a snapshot.
                let snapshot = Message::Snapshot(
                    self.signed_endpoints
                        .values()
                        .into_iter()
                        .cloned()
                        .collect(),
                );
                self.send_message(node_id, snapshot);

                message_sender(receiver, sink).event(move |result| Event::OutgoingFailed {
                    node_id,
                    attempt_count: 0, // reset to 0, since we have had a successful connection
                    error: result.err().map(Into::into),
                })
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
        }
    }

    // TODO: Move to trait.
    /// Queue a payload message to be sent to a specific node.
    #[inline]
    pub fn send(&self, dest: Fingerprint, payload: P) {
        self.send_message(dest, Message::Payload(payload))
    }

    /// Queue a broadcast to all connected nodes.
    #[inline]
    pub fn broadcast(&self, payload: P) {
        self.broadcast_message(Message::Payload(payload))
    }

    /// Queue a message to be sent to all nodes.
    fn broadcast_message(&self, msg: Message<P>) {
        for node_id in self.outgoing.keys() {
            self.send_message(*node_id, msg.clone());
        }
    }

    /// Queue a message to be sent to a specific node.
    fn send_message(&self, dest: Fingerprint, msg: Message<P>) {
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

    /// Update the internal endpoint store from a given endpoint.
    ///
    /// Returns the node id of the endpoint if it was new.
    #[inline]
    fn update_endpoint(&mut self, endpoint: &Endpoint) -> Option<Fingerprint> {
        let fp = endpoint
            .cert
            .public_key_fingerprint()
            .expect("FIXME: this should be infallible");

        let mut rv = None;
        self.endpoints.entry(fp).and_modify(|prev| {
            if endpoint > prev {
                rv = Some(fp);
                *prev = endpoint.clone();
            }
        });

        rv
    }

    /// Update internal endpoint store and if new, output a `BroadcastEndpoint` effect.
    #[inline]
    fn update_and_broadcast_if_new(
        &mut self,
        signed: Signed<Endpoint>,
    ) -> Multiple<Effect<Event<P>>> {
        match signed.validate_self_signed(|endpoint| Ok(endpoint.cert.public_key())) {
            Ok(endpoint) => {
                if let Some(node_id) = self.update_endpoint(&endpoint) {
                    // We learned of a new endpoint. We store it and note whether it is the first
                    // endpoint for the node.
                    self.signed_endpoints.insert(node_id, signed.clone());
                    self.endpoints.insert(node_id, endpoint.clone());

                    self.broadcast_message(Message::BroadcastEndpoint(signed));

                    if self.outgoing.remove(&node_id).is_none() {
                        info!(%node_id, ?endpoint, "new outgoing channel");
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

                        info!(%node_id, ?endpoint, "endpoint changed");
                        Multiple::new()
                    }
                } else {
                    Multiple::new()
                }
            }
            Err(err) => {
                warn!(%err, ?signed, "received invalid endpoint");
                Multiple::new()
            }
        }
    }

    /// Handle received message.
    // Internal function to keep indentation and nesting sane.
    fn handle_message(
        &mut self,
        node_id: Fingerprint,
        msg: Message<P>,
    ) -> Multiple<Effect<Event<P>>> {
        debug!(%node_id, ?msg, "incoming msg");
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

/// Determine bind address for now.
///
/// Will attempt to bind on the root address first if the `bind_interface` is the same as the
/// interface of `root_addr`. Otherwise uses an unused port on `bind_interface`.
async fn create_listener(cfg: &config::SmallNetwork) -> io::Result<tokio::net::TcpListener> {
    if cfg.root_addr.ip() == cfg.bind_interface {
        // Try to become the root node, if the root nodes interface is available.
        match tokio::net::TcpListener::bind(cfg.root_addr).await {
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
    Ok(tokio::net::TcpListener::bind((cfg.bind_interface, 0u16)).await?)
}

/// Core accept loop for the networking server.
///
/// Never terminates.
async fn server_task<P, R: reactor::Reactor>(
    eq: reactor::EventQueueHandle<R, Event<P>>,
    mut listener: tokio::net::TcpListener,
) {
    loop {
        // We handle accept errors here, since they can be caused by a temporary
        // resource shortage or the remote side closing the connection while
        // it is waiting in the queue.
        match listener.accept().await {
            Ok((stream, addr)) => {
                // Move the incoming connection to the event queue for handling.
                let ev = Event::IncomingNew { stream, addr };
                eq.schedule(ev, reactor::Queue::NetworkIncoming).await;
            }
            Err(err) => warn!(%err, "dropping incoming connection during accept"),
        }
    }
}

/// Server-side TLS handshake
///
/// This function groups the TLS handshake into a convenient function, enabling the `?` operator.
async fn setup_tls(
    stream: tokio::net::TcpStream,
    cert: Arc<x509::X509>,
    private_key: Arc<pkey::PKey<pkey::Private>>,
) -> anyhow::Result<(Fingerprint, Transport)> {
    let tls_stream = tokio_openssl::accept(
        &tls::create_tls_acceptor(&cert.as_ref(), &private_key.as_ref())?,
        stream,
    )
    .await?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or_else(|| anyhow::anyhow!("no peer certificate presented"))?;

    Ok((tls::validate_cert(&peer_cert.as_ref())?, tls_stream))
}

/// Network message reader.
///
/// Schedules all received messages until the stream is closed or an error occurred.
async fn message_reader<R, P>(
    eq: reactor::EventQueueHandle<R, Event<P>>,
    mut stream: futures::stream::SplitStream<FramedTransport<P>>,
    node_id: Fingerprint,
) -> io::Result<()>
where
    R: reactor::Reactor,
    P: DeserializeOwned + Send,
{
    while let Some(msg_result) = stream.next().await {
        match msg_result {
            Ok(msg) => {
                // We've received a message, push it to the reactor.
                eq.schedule(
                    Event::IncomingMessage { node_id, msg },
                    reactor::Queue::NetworkIncoming,
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

/// Network message sender
///
/// Reads from a channel and sends all messages, until the stream is closed or an error occured.
async fn message_sender<P>(
    mut queue: mpsc::UnboundedReceiver<Message<P>>,
    mut sink: futures::stream::SplitSink<FramedTransport<P>, Message<P>>,
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
type Transport = tokio_openssl::SslStream<tokio::net::TcpStream>;

/// A framed transport for `Message`s.
type FramedTransport<P> = tokio_serde::SymmetricallyFramed<
    tokio_util::codec::Framed<Transport, tokio_util::codec::LengthDelimitedCodec>,
    Message<P>,
    tokio_serde::formats::SymmetricalMessagePack<Message<P>>,
>;

/// Construct a new framed transport on a stream.
fn framed<P>(stream: Transport) -> FramedTransport<P> {
    let length_delimited =
        tokio_util::codec::Framed::new(stream, tokio_util::codec::LengthDelimitedCodec::new());
    tokio_serde::SymmetricallyFramed::new(
        length_delimited,
        tokio_serde::formats::SymmetricalMessagePack::<Message<P>>::default(),
    )
}

/// Initiate a TLS connection to an endpoint.
async fn connect_outgoing(
    endpoint: Endpoint,
    cert: Arc<x509::X509>,
    private_key: Arc<pkey::PKey<pkey::Private>>,
) -> anyhow::Result<Transport> {
    let mut config = tls::create_tls_connector(&cert, &private_key)
        .context("could not create TLS connector")?
        .configure()?;
    config.set_verify_hostname(false);

    let stream = tokio::net::TcpStream::connect(endpoint.addr)
        .await
        .context("TCP connection failed")?;

    let tls_stream = tokio_openssl::connect(config, "this-will-not-be-checked.example.com", stream)
        .await
        .context("tls handshake failed")?;

    let server_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or_else(|| anyhow::anyhow!("no server certificate presented"))?;

    let remote_id = tls::validate_cert(&server_cert.as_ref())?;

    if remote_id != endpoint.cert.public_key_fingerprint()? {
        anyhow::bail!("remote node has wrong ID");
    }

    Ok(tls_stream)
}

// Impose a total ordering on endpoints. Compare timestamps first, if the same, order by actual
// address. If both of these are the same, use the TLS certificate's fingerprint as a tie-breaker.
impl Ord for Endpoint {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        Ord::cmp(&self.timestamp_ns, &other.timestamp_ns)
            .then_with(|| {
                Ord::cmp(
                    &(self.addr.ip(), self.addr.port()),
                    &(other.addr.ip(), other.addr.port()),
                )
            })
            .then_with(|| Ord::cmp(&self.cert.fingerprint(), &other.cert.fingerprint()))
    }
}
impl PartialOrd for Endpoint {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<R, P> fmt::Debug for SmallNetwork<R, P>
where
    R: reactor::Reactor,
    P: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
