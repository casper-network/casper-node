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
//! The network itself is best-effort, during regular operation, no messages should be lost.
//!
//! # Connection
//!
//! Every node has an ID and a public listening address. The objective of each node is to constantly
//! maintain an outgoing connection to each other node (and thus have an incoming connection from
//! these nodes as well).
//!
//! Any incoming connection is strictly read from, while any outgoing connection is strictly used
//! for sending messages.
//!
//! Nodes gossip their public listening addresses periodically, and on learning of a new address,
//! a node will try to establish an outgoing connection.
//!
//! On losing an incoming or outgoing connection for a given peer, the other connection is closed.
//! No explicit reconnect is attempted. Instead, if the peer is still online, the normal gossiping
//! process will cause both peers to connect again.

mod config;
mod error;
mod event;
mod message;
#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    io,
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use anyhow::Context;
use futures::{
    future::{select, BoxFuture, Either},
    stream::{SplitSink, SplitStream},
    FutureExt, SinkExt, StreamExt,
};
use openssl::pkey;
use pkey::{PKey, Private};
use rand::{seq::IteratorRandom, CryptoRng, Rng};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
    task::JoinHandle,
};
use tokio_openssl::SslStream;
use tokio_serde::{formats::SymmetricalMessagePack, SymmetricallyFramed};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info, warn, Span};

use self::error::Result;
pub(crate) use self::{event::Event, message::Message};
use crate::{
    components::Component,
    effect::{
        announcements::NetworkAnnouncement, requests::NetworkRequest, EffectBuilder, EffectExt,
        EffectResultExt, Effects,
    },
    reactor::{EventQueueHandle, Finalize, QueueKind},
    tls::{self, KeyFingerprint, TlsCert},
};
// Seems to be a false positive.
#[allow(unreachable_pub)]
pub use config::Config;
// Seems to be a false positive.
#[allow(unreachable_pub)]
pub use error::Error;

/// A node ID.
///
/// The key fingerprint found on TLS certificates.
pub(crate) type NodeId = KeyFingerprint;

pub(crate) struct SmallNetwork<REv: 'static, P> {
    /// Server certificate.
    certificate: Arc<TlsCert>,
    /// Server secret key.
    secret_key: Arc<PKey<Private>>,
    /// Our public listening address.
    public_address: SocketAddr,
    /// Handle to event queue.
    event_queue: EventQueueHandle<REv>,
    /// Incoming network connection addresses.
    incoming: HashMap<NodeId, SocketAddr>,
    /// Outgoing network connections' messages.
    outgoing: HashMap<NodeId, UnboundedSender<Message<P>>>,
    /// Channel signaling a shutdown of the small network.
    // Note: This channel never sends anything, instead it is closed when `SmallNetwork` is dropped,
    //       signalling the receiver that it should cease operation. Don't listen to clippy!
    #[allow(dead_code)]
    shutdown: Option<oneshot::Sender<()>>,
    /// Join handle for the server thread.
    #[allow(dead_code)]
    server_join_handle: Option<JoinHandle<()>>,
}

impl<REv, P> SmallNetwork<REv, P>
where
    P: Serialize + DeserializeOwned + Clone + Debug + Send + 'static,
    REv: Send + From<Event<P>>,
{
    #[allow(clippy::type_complexity)]
    pub(crate) fn new(
        event_queue: EventQueueHandle<REv>,
        cfg: Config,
    ) -> Result<(SmallNetwork<REv, P>, Effects<Event<P>>)> {
        let span = tracing::debug_span!("net");
        let _enter = span.enter();

        let server_span = tracing::info_span!("server");

        // First, we generate the TLS keys.
        let (cert, secret_key) = tls::generate_node_cert().map_err(Error::CertificateGeneration)?;
        let certificate = Arc::new(tls::validate_cert(cert).map_err(Error::OwnCertificateInvalid)?);

        // We can now create a listener.
        let bind_address = SocketAddr::new(cfg.bind_interface(), cfg.bind_port());
        let listener = TcpListener::bind(bind_address)
            .map_err(|error| Error::ListenerCreation(error, bind_address))?;
        let local_address = listener.local_addr().map_err(Error::ListenerAddr)?;
        let public_address = SocketAddr::new(cfg.public_ip(), local_address.port());

        // Run the server task.
        // We spawn it ourselves instead of through an effect to get a hold of the join handle,
        // which we need to shutdown cleanly later on.
        let node_id = certificate.public_key_fingerprint();
        info!(%node_id, %local_address, %public_address, "starting server background task");
        let (server_shutdown_sender, server_shutdown_receiver) = oneshot::channel();
        let server_join_handle = tokio::spawn(server_task(
            event_queue,
            tokio::net::TcpListener::from_std(listener).map_err(Error::ListenerConversion)?,
            server_shutdown_receiver,
            server_span,
        ));

        let model = SmallNetwork {
            public_address,
            certificate,
            secret_key: Arc::new(secret_key),
            event_queue,
            incoming: HashMap::new(),
            outgoing: HashMap::new(),
            shutdown: Some(server_shutdown_sender),
            server_join_handle: Some(server_join_handle),
        };

        // Connect to the known node if available.
        let effects = if let Some(known_address) = cfg.known_address() {
            connect_outgoing(
                known_address,
                Arc::clone(&model.certificate),
                Arc::clone(&model.secret_key),
            )
            .result(
                move |(node_id, transport)| Event::OutgoingEstablished { node_id, transport },
                move |error| Event::BootstrappingFailed { error },
            )
        } else {
            debug!("no known nodes to connect to");
            Effects::new()
        };

        Ok((model, effects))
    }

    /// Queues a message to be sent to all nodes.
    fn broadcast_message(&self, msg: Message<P>) {
        for node_id in self.outgoing.keys() {
            self.send_message(*node_id, msg.clone());
        }
    }

    /// Queues a message to `count` random nodes on the network.
    fn gossip_message<R: Rng + ?Sized>(
        &self,
        rng: &mut R,
        msg: Message<P>,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let node_ids = self
            .outgoing
            .keys()
            .filter(|&node_id| !exclude.contains(node_id))
            .choose_multiple(rng, count);

        if node_ids.len() != count {
            warn!(
                wanted = count,
                selected = node_ids.len(),
                "could not select enough random nodes for gossiping, not enough non-excluded \
                outgoing connections"
            );
        }

        for &node_id in &node_ids {
            self.send_message(*node_id, msg.clone());
        }

        node_ids.into_iter().copied().collect()
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

    /// Sets up an established outgoing connection.
    fn setup_outgoing(&mut self, node_id: NodeId, transport: Transport) -> Effects<Event<P>> {
        // This connection is send-only, we only use the sink.
        let peer_address = transport
            .get_ref()
            .peer_addr()
            .expect("should have peer address");
        let (sink, _stream) = framed::<P>(transport).split();
        debug!(peer=%node_id, %peer_address, "established outgoing connection");

        let (sender, receiver) = mpsc::unbounded_channel();
        if self.outgoing.insert(node_id, sender).is_some() {
            // We assume that for a reconnect to have happened, the outgoing entry must have
            // been either non-existent yet or cleaned up by the handler of the connection
            // closing event. If this is not the case, an assumed invariant has been violated.
            error!(%node_id, "did not expect leftover channel in outgoing map");
        }

        message_sender(receiver, sink).event(move |result| Event::OutgoingFailed {
            node_id,
            error: result.err().map(Into::into),
        })
    }

    fn remove(&mut self, node_id: &NodeId) {
        let _ = self.incoming.remove(&node_id);
        let _ = self.outgoing.remove(&node_id);
    }

    /// Handles a received message.
    fn handle_message(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        node_id: NodeId,
        msg: Message<P>,
    ) -> Effects<Event<P>>
    where
        REv: From<NetworkAnnouncement<NodeId, P>>,
    {
        effect_builder
            .announce_message_received(node_id, msg.0)
            .ignore()
    }

    /// Returns the set of connected nodes.
    #[cfg(test)]
    pub(crate) fn connected_nodes(&self) -> HashSet<NodeId> {
        self.outgoing.keys().cloned().collect()
    }

    /// Returns the node id of this network node.
    #[cfg(test)]
    pub(crate) fn node_id(&self) -> NodeId {
        self.certificate.public_key_fingerprint()
    }
}

impl<REv, P> Finalize for SmallNetwork<REv, P>
where
    REv: Send + 'static,
    P: Send + 'static,
{
    fn finalize(mut self) -> BoxFuture<'static, ()> {
        async move {
            // Close the shutdown socket, causing the server to exit.
            drop(self.shutdown.take());

            // Wait for the server to exit cleanly.
            if let Some(join_handle) = self.server_join_handle.take() {
                match join_handle.await {
                    Ok(_) => debug!("server exited cleanly"),
                    Err(err) => error!(%err, "could not join server task cleanly"),
                }
            } else {
                warn!("server shutdown while already shut down")
            }
        }
        .boxed()
    }
}

impl<REv, R, P> Component<REv, R> for SmallNetwork<REv, P>
where
    REv: Send + From<Event<P>> + From<NetworkAnnouncement<NodeId, P>>,
    R: Rng + CryptoRng + ?Sized,
    P: Serialize + DeserializeOwned + Clone + Debug + Display + Send + 'static,
{
    type Event = Event<P>;

    #[allow(clippy::cognitive_complexity)]
    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::BootstrappingFailed { error } => {
                error!(%error, "connection to known node failed");
                panic!("failed to connect to known node");
            }
            Event::IncomingNew { stream, address } => {
                debug!(%address, "incoming connection, starting TLS handshake");

                setup_tls(stream, self.certificate.clone(), self.secret_key.clone())
                    .boxed()
                    .event(move |result| Event::IncomingHandshakeCompleted { result, address })
            }
            Event::IncomingHandshakeCompleted { result, address } => {
                match result {
                    Ok((node_id, transport)) => {
                        debug!(peer=%node_id, %address, "established incoming connection");
                        // The sink is never used, as we only read data from incoming connections.
                        let (_sink, stream) = framed::<P>(transport).split();

                        let _ = self.incoming.insert(node_id, address);

                        message_reader(self.event_queue, stream, node_id).event(move |result| {
                            Event::IncomingClosed {
                                result,
                                node_id,
                                address,
                            }
                        })
                    }
                    Err(err) => {
                        warn!(%address, %err, "TLS handshake failed");
                        Effects::new()
                    }
                }
            }
            Event::IncomingMessage { node_id, msg } => {
                self.handle_message(effect_builder, node_id, msg)
            }
            Event::IncomingClosed {
                result,
                node_id,
                address,
            } => {
                match result {
                    Ok(()) => info!(%node_id, %address, "connection closed"),
                    Err(err) => warn!(%node_id, %address, %err, "connection dropped"),
                }
                self.remove(&node_id);
                Effects::new()
            }
            Event::OutgoingEstablished { node_id, transport } => {
                self.setup_outgoing(node_id, transport)
            }
            Event::OutgoingFailed { node_id, error } => {
                if let Some(err) = error {
                    warn!(%node_id, %err, "outgoing connection failed");
                } else {
                    warn!(%node_id, "outgoing connection closed");
                }
                self.remove(&node_id);
                Effects::new()
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
                self.send_message(dest, Message(payload));
                responder.respond(()).ignore()
            }
            Event::NetworkRequest {
                req: NetworkRequest::Broadcast { payload, responder },
            } => {
                // We're given a message to broadcast.
                self.broadcast_message(Message(payload));
                responder.respond(()).ignore()
            }
            Event::NetworkRequest {
                req:
                    NetworkRequest::Gossip {
                        payload,
                        count,
                        exclude,
                        responder,
                    },
            } => {
                // We're given a message to gossip.
                let sent_to = self.gossip_message(rng, Message(payload), count, exclude);
                responder.respond(sent_to).ignore()
            }
        }
    }
}

/// Core accept loop for the networking server.
///
/// Never terminates.
async fn server_task<P, REv>(
    event_queue: EventQueueHandle<REv>,
    mut listener: tokio::net::TcpListener,
    shutdown: oneshot::Receiver<()>,
    span: Span,
) where
    REv: From<Event<P>>,
{
    let _enter = span.enter();

    // The server task is a bit tricky, since it has to wait on incoming connections while at the
    // same time shut down if the networking component is dropped, otherwise the TCP socket will
    // stay open, preventing reuse.

    // We first create a future that never terminates, handling incoming connections:
    let accept_connections = async move {
        loop {
            // We handle accept errors here, since they can be caused by a temporary resource
            // shortage or the remote side closing the connection while it is waiting in
            // the queue.
            match listener.accept().await {
                Ok((stream, address)) => {
                    // Move the incoming connection to the event queue for handling.
                    let event = Event::IncomingNew { stream, address };
                    event_queue
                        .schedule(event, QueueKind::NetworkIncoming)
                        .await;
                }
                // TODO: Handle resource errors gracefully.
                //       In general, two kinds of errors occur here: Local resource exhaustion,
                //       which should be handled by waiting a few milliseconds, or remote connection
                //       errors, which can be dropped immediately.
                //
                //       The code in its current state will consume 100% CPU if local resource
                //       exhaustion happens, as no distinction is made and no delay introduced.
                Err(err) => warn!(%err, "dropping incoming connection during accept"),
            }
        }
    };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match select(shutdown, Box::pin(accept_connections)).await {
        Either::Left(_) => info!("shutting down socket, no longer accepting incoming connections"),
        Either::Right(_) => unreachable!(),
    }
}

/// Server-side TLS handshake.
///
/// This function groups the TLS handshake into a convenient function, enabling the `?` operator.
async fn setup_tls(
    stream: TcpStream,
    cert: Arc<TlsCert>,
    secret_key: Arc<PKey<Private>>,
) -> Result<(NodeId, Transport)> {
    let tls_stream = tokio_openssl::accept(
        &tls::create_tls_acceptor(&cert.as_x509().as_ref(), &secret_key.as_ref())
            .map_err(Error::AcceptorCreation)?,
        stream,
    )
    .await?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or_else(|| Error::NoClientCertificate)?;

    Ok((
        tls::validate_cert(peer_cert)?.public_key_fingerprint(),
        tls_stream,
    ))
}

/// Network message reader.
///
/// Schedules all received messages until the stream is closed or an error occurs.
async fn message_reader<REv, P>(
    event_queue: EventQueueHandle<REv>,
    mut stream: SplitStream<FramedTransport<P>>,
    node_id: NodeId,
) -> io::Result<()>
where
    P: DeserializeOwned + Send + Display,
    REv: From<Event<P>>,
{
    while let Some(msg_result) = stream.next().await {
        match msg_result {
            Ok(msg) => {
                debug!(%msg, %node_id, "message received");
                // We've received a message, push it to the reactor.
                event_queue
                    .schedule(
                        Event::IncomingMessage { node_id, msg },
                        QueueKind::NetworkIncoming,
                    )
                    .await;
            }
            Err(err) => {
                warn!(%err, peer=%node_id, "receiving message failed, closing connection");
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
) -> Result<()>
where
    P: Serialize + Send,
{
    while let Some(payload) = queue.recv().await {
        // We simply error-out if the sink fails, it means that our connection broke.
        sink.send(payload).await.map_err(Error::MessageNotSent)?;
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

/// Initiates a TLS connection to a remote address.
async fn connect_outgoing(
    peer_address: SocketAddr,
    our_certificate: Arc<TlsCert>,
    secret_key: Arc<PKey<Private>>,
) -> Result<(NodeId, Transport)> {
    let mut config = tls::create_tls_connector(&our_certificate.as_x509(), &secret_key)
        .context("could not create TLS connector")?
        .configure()
        .map_err(Error::ConnectorConfiguration)?;
    config.set_verify_hostname(false);

    let stream = tokio::net::TcpStream::connect(peer_address)
        .await
        .context("TCP connection failed")?;

    let tls_stream = tokio_openssl::connect(config, "this-will-not-be-checked.example.com", stream)
        .await
        .context("tls handshake failed")?;

    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or_else(|| Error::NoServerCertificate)?;

    let peer_id = tls::validate_cert(peer_cert)?.public_key_fingerprint();
    Ok((peer_id, tls_stream))
}

impl<R, P> Debug for SmallNetwork<R, P>
where
    P: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmallNetwork")
            .field("certificate", &"<SSL cert>")
            .field("secret_key", &"<hidden>")
            .field("public_address", &self.public_address)
            .field("event_queue", &"<event_queue>")
            .field("outgoing", &self.outgoing)
            .finish()
    }
}
