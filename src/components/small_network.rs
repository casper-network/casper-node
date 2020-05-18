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

use crate::effect::Effect;
use crate::effect::EffectExt;
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
use tracing::{debug, info, warn};

#[derive(Debug, Deserialize, Serialize)]
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

    // From external effects.
    /// Send a message to a specific node if it is connected.
    SendMessage { dest: Fingerprint, payload: P },
    /// Send a message to all currently connected nodes.
    Broadcast { payload: P },
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
    outgoing: collections::HashMap<Fingerprint, mpsc::UnboundedSender<P>>,
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

        // Run the server task.
        info!(?addr, "starting server background task");

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
        let effects = server_task(eq, listener).boxed().ignore();
        Ok((model, effects))
    }

    pub fn handle_event(&mut self, ev: Event<P>) -> Multiple<Effect<Event<P>>> {
        match ev {
            Event::IncomingNew { stream, addr } => {
                // We have received a new incoming connection, now setup a TLS context.
                debug!(%addr, "Incoming connection, starting TLS handshake");

                setup_tls(stream, self.cert.clone(), self.private_key.clone())
                    .boxed()
                    .event(move |result| Event::IncomingHandshakeCompleted { result, addr })
            }
            Event::IncomingHandshakeCompleted {
                result: Ok((fp, tls_stream)),
                addr,
            } => {
                // The sink is never used, as we only read data from incoming connections.
                let (_sink, stream) = framed::<P>(tls_stream).split();

                // Start the reader.
                message_reader(self.eq, stream, fp)
                    .event(move |result| Event::IncomingClosed { result, addr })
            }
            Event::IncomingHandshakeCompleted {
                result: Err(err),
                addr,
            } => {
                warn!(%addr, %err, "TLS handshake failed");
                Multiple::new()
            }
            Event::IncomingMessage { node_id, msg } => self.handle_message(node_id, msg),
            Event::IncomingClosed {
                result: Ok(()),
                addr,
            } => {
                info!(%addr, "connection closed");
                Multiple::new()
            }
            Event::IncomingClosed {
                result: Err(err),
                addr,
            } => {
                warn!(%addr, %err, "connection dopped");
                Multiple::new()
            }
            Event::Broadcast { payload } => {
                // Send to all connected nodes.
                for node_id in self.outgoing.keys() {
                    self.send_message(*node_id, payload.clone());
                }
                Multiple::new()
            }
            Event::SendMessage { dest, payload } => {
                self.send_message(dest, payload);
                Multiple::new()
            }
        }
    }

    fn send_message(&self, dest: Fingerprint, payload: P) {
        // Try to send the message.
        if let Some(sender) = self.outgoing.get(&dest) {
            if let Err(payload) = sender.send(payload) {
                // We lost the connection, but that fact has not reached us yet.
                warn!(?payload, "dropped outgoing message, lost connection");
            }
        } else {
            // We are not connected, so the reconnection is likely already in progress.
            warn!(?payload, "dropped outgoing message, no connection");
        }
    }

    /// Update the internal endpoint store from a given endpoint.
    ///
    /// Returns the node id of the endpoint if it was new.
    #[inline]
    fn update_endpoint(&mut self, endpoint: &Endpoint) -> Option<Fingerprint> {
        let fp = endpoint.cert.public_key_fingerprint().expect("FIXME");

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
                if let Some(fp) = self.update_endpoint(&endpoint) {
                    // We learned of a new endpoint. Ensure it is sent to all connected nodes and
                    // store it for later use.
                    self.signed_endpoints.insert(fp, signed.clone());

                    broadcast::<P>(Message::BroadcastEndpoint(signed)).ignore()
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

// Stand-in for message sending effect.
async fn broadcast<P>(msg: Message<P>) {
    std::mem::drop(msg);
    todo!()
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
