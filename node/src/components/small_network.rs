//! Fully connected overlay network
//!
//! The *small network* is an overlay network where each node participating is attempting to
//! maintain a connection to every other node identified on the same network. The component does not
//! guarantee message delivery, so in between reconnections, messages may be lost.
//!
//! # Node IDs
//!
//! Each node has a self-generated node ID based on its self-signed TLS certificate. Whenever a
//! connection is made to another node, it verifies the "server"'s certificate to check that it
//! connected to a valid node and sends its own certificate during the TLS handshake, establishing
//! identity.
//!
//! # Connection
//!
//! Every node has an ID and a public listening address. The objective of each node is to constantly
//! maintain an outgoing connection to each other node (and thus have an incoming connection from
//! these nodes as well).
//!
//! Any incoming connection is, after a handshake process, strictly read from, while any outgoing
//! connection is strictly used for sending messages, also after a handshake.
//!
//! Nodes gossip their public listening addresses periodically, and will try to establish and
//! maintain an outgoing connection to any new address learned.

mod chain_info;
mod config;
mod counting_format;
mod error;
mod event;
mod gossiped_address;
mod message;
mod message_pack_format;
mod outgoing;
mod symmetry;
pub(crate) mod tasks;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    fmt::{self, Debug, Display, Formatter},
    io, mem,
    net::{SocketAddr, TcpListener},
    result,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt, StreamExt};
use openssl::{error::ErrorStack as OpenSslErrorStack, pkey};
use pkey::{PKey, Private};
use prometheus::Registry;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{self, UnboundedSender},
        watch,
    },
    task::JoinHandle,
};
use tokio_openssl::SslStream;
use tokio_serde::SymmetricallyFramed;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info, trace, warn, Instrument, Span};

use self::{
    counting_format::{ConnectionId, CountingFormat, Role},
    error::Result,
    event::IncomingConnection,
    message_pack_format::MessagePackFormat,
    outgoing::{DialOutcome, DialRequest, OutgoingConfig, OutgoingManager},
    symmetry::ConnectionSymmetry,
    tasks::NetworkContext,
};
pub(crate) use self::{
    error::display_error,
    event::Event,
    gossiped_address::GossipedAddress,
    message::{Message, MessageKind, Payload},
};
use crate::{
    components::{networking_metrics::NetworkingMetrics, Component},
    effect::{
        announcements::{BlocklistAnnouncement, NetworkAnnouncement},
        requests::{NetworkInfoRequest, NetworkRequest},
        EffectBuilder, EffectExt, EffectResultExt, Effects,
    },
    reactor::{EventQueueHandle, Finalize, ReactorEvent},
    tls::{self, TlsCert, ValidationError},
    types::NodeId,
    utils, NodeRng,
};
use chain_info::ChainInfo;
pub use config::Config;
pub use error::Error;

const MAX_ASYMMETRIC_TIME: Duration = Duration::from_secs(60);

const MAX_METRICS_DROP_ATTEMPTS: usize = 25;
const DROP_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Duration peers are kept on the block list, before being redeemed.
const BLOCKLIST_RETAIN_DURATION: Duration = Duration::from_secs(60 * 10);

/// How often to keep attempting to reconnect to a node before giving up. Note that reconnection
/// delays increase exponentially!
const RECONNECTION_ATTEMPTS: u8 = 8;

/// Basic reconnection timeout.
///
/// The first reconnection attempt will be made after 2x this timeout.
const BASE_RECONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

/// Interval during which to perform outgoing manager housekeeping.
const OUTGOING_MANAGER_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// Interval for checking for symmetrical connections.
const SYMMETRY_SWEEP_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, DataSize, Debug)]
pub struct OutgoingConnection<P> {
    #[data_size(skip)] // Unfortunately, there is no way to inspect an `UnboundedSender`.
    sender: UnboundedSender<Message<P>>,
    peer_addr: SocketAddr,
}

impl<P> Display for OutgoingConnection<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "outgoing connection to {}", self.peer_addr)
    }
}

#[derive(DataSize)]
pub(crate) struct SmallNetwork<REv, P>
where
    REv: 'static,
    P: Payload,
{
    /// Initial configuration values.
    cfg: Config,
    /// Read-only networking information shared across tasks.
    context: Arc<NetworkContext<REv>>,

    /// Outgoing connections manager.
    outgoing_manager: OutgoingManager<OutgoingConnection<P>, Error>,
    /// Tracks whether a connection is symmetric or not.
    connection_symmetries: HashMap<NodeId, ConnectionSymmetry>,

    /// Channel signaling a shutdown of the small network.
    // Note: This channel is closed when `SmallNetwork` is dropped, signalling the receivers that
    // they should cease operation.
    #[data_size(skip)]
    shutdown_sender: Option<watch::Sender<()>>,
    /// A clone of the receiver is passed to the message reader for all new incoming connections in
    /// order that they can be gracefully terminated.
    #[data_size(skip)]
    shutdown_receiver: watch::Receiver<()>,
    /// Join handle for the server thread.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,

    /// Networking metrics.
    #[data_size(skip)]
    net_metrics: Arc<NetworkingMetrics>,
}

impl<REv, P> SmallNetwork<REv, P>
where
    P: Payload + 'static,
    REv: ReactorEvent + From<Event<P>> + From<NetworkAnnouncement<NodeId, P>>,
{
    /// Creates a new small network component instance.
    #[allow(clippy::type_complexity)]
    pub(crate) fn new<C: Into<ChainInfo>>(
        event_queue: EventQueueHandle<REv>,
        cfg: Config,
        registry: &Registry,
        small_network_identity: SmallNetworkIdentity,
        chain_info_source: C,
    ) -> Result<(SmallNetwork<REv, P>, Effects<Event<P>>)> {
        let mut known_addresses = HashSet::new();
        for address in &cfg.known_addresses {
            match utils::resolve_address(address) {
                Ok(known_address) => {
                    if !known_addresses.insert(known_address) {
                        warn!(%address, resolved=%known_address, "ignoring duplicated known address");
                    };
                }
                Err(ref err) => {
                    warn!(%address, err=display_error(err), "failed to resolve known address");
                }
            }
        }

        // Assert we have at least one known address in the config.
        if known_addresses.is_empty() {
            warn!("no known addresses provided via config or all failed DNS resolution");
            return Err(Error::InvalidConfig);
        }

        let outgoing_manager = OutgoingManager::new(OutgoingConfig {
            retry_attempts: RECONNECTION_ATTEMPTS,
            base_timeout: BASE_RECONNECTION_TIMEOUT,
            unblock_after: BLOCKLIST_RETAIN_DURATION,
            sweep_timeout: cfg.max_addr_pending_time.into(),
        });

        let mut public_addr =
            utils::resolve_address(&cfg.public_address).map_err(Error::ResolveAddr)?;

        let net_metrics = Arc::new(NetworkingMetrics::new(&registry)?);

        // We can now create a listener.
        let bind_address = utils::resolve_address(&cfg.bind_address).map_err(Error::ResolveAddr)?;
        let listener = TcpListener::bind(bind_address)
            .map_err(|error| Error::ListenerCreation(error, bind_address))?;
        // We must set non-blocking to `true` or else the tokio task hangs forever.
        listener
            .set_nonblocking(true)
            .map_err(Error::ListenerSetNonBlocking)?;

        let local_addr = listener.local_addr().map_err(Error::ListenerAddr)?;

        // Substitute the actually bound port if set to 0.
        if public_addr.port() == 0 {
            public_addr.set_port(local_addr.port());
        }

        let context = Arc::new(NetworkContext {
            event_queue,
            our_id: NodeId::from(&small_network_identity),
            our_cert: small_network_identity.tls_certificate,
            secret_key: small_network_identity.secret_key,
            net_metrics: Arc::downgrade(&net_metrics),
            chain_info: chain_info_source.into(),
            public_addr,
        });

        // Run the server task.
        // We spawn it ourselves instead of through an effect to get a hold of the join handle,
        // which we need to shutdown cleanly later on.
        info!(%local_addr, %public_addr, "starting server background task");

        let (server_shutdown_sender, server_shutdown_receiver) = watch::channel(());
        let shutdown_receiver = server_shutdown_receiver.clone();
        let server_join_handle = tokio::spawn(tasks::server(
            context.clone(),
            tokio::net::TcpListener::from_std(listener).map_err(Error::ListenerConversion)?,
            server_shutdown_receiver,
        ));

        let mut component = SmallNetwork {
            cfg,
            context,
            outgoing_manager,
            connection_symmetries: HashMap::new(),
            shutdown_sender: Some(server_shutdown_sender),
            shutdown_receiver,
            server_join_handle: Some(server_join_handle),
            net_metrics,
        };

        let effect_builder = EffectBuilder::new(event_queue);

        // Learn all known addresses and mark them as unforgettable.
        let now = Instant::now();
        let dial_requests: Vec<_> = known_addresses
            .into_iter()
            .filter_map(|addr| component.outgoing_manager.learn_addr(addr, true, now))
            .collect();
        let mut effects = component.process_dial_requests(dial_requests);

        // Start broadcasting our public listening address.
        effects.extend(
            effect_builder
                .set_timeout(component.cfg.initial_gossip_delay.into())
                .event(|_| Event::GossipOurAddress),
        );

        // Start regular housekeeping of the outgoing connections.
        effects.extend(
            effect_builder
                .set_timeout(OUTGOING_MANAGER_SWEEP_INTERVAL)
                .event(|_| Event::SweepOutgoing),
        );

        Ok((component, effects))
    }

    /// Queues a message to be sent to all nodes.
    fn broadcast_message(&self, msg: Message<P>) {
        for peer_id in self.outgoing_manager.connected_peers() {
            self.send_message(peer_id, msg.clone());
        }
    }

    /// Queues a message to `count` random nodes on the network.
    fn gossip_message(
        &self,
        rng: &mut NodeRng,
        msg: Message<P>,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let peer_ids = self
            .outgoing_manager
            .connected_peers()
            .filter(|peer_id| !exclude.contains(peer_id))
            .choose_multiple(rng, count);

        if peer_ids.len() != count {
            // TODO - set this to `warn!` once we are normally testing with networks large enough to
            //        make it a meaningful and infrequent log message.
            trace!(
                our_id=%self.context.our_id,
                wanted = count,
                selected = peer_ids.len(),
                "could not select enough random nodes for gossiping, not enough non-excluded \
                outgoing connections"
            );
        }

        for &peer_id in &peer_ids {
            self.send_message(peer_id, msg.clone());
        }

        peer_ids.into_iter().collect()
    }

    /// Queues a message to be sent to a specific node.
    fn send_message(&self, dest: NodeId, msg: Message<P>) {
        // Try to send the message.
        if let Some(connection) = self.outgoing_manager.get_route(dest) {
            if let Err(msg) = connection.sender.send(msg) {
                // We lost the connection, but that fact has not reached us yet.
                warn!(our_id=%self.context.our_id, %dest, ?msg, "dropped outgoing message, lost connection");
            } else {
                self.net_metrics.queued_messages.inc();
            }
        } else {
            // We are not connected, so the reconnection is likely already in progress.
            debug!(our_id=%self.context.our_id, %dest, ?msg, "dropped outgoing message, no connection");
        }
    }

    #[allow(clippy::redundant_clone)]
    fn handle_incoming_connection(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        incoming: Box<IncomingConnection<P>>,
        span: Span,
    ) -> Effects<Event<P>> {
        span.clone().in_scope(|| match *incoming {
            IncomingConnection::FailedEarly {
                peer_addr: _,
                ref error,
            } => {
                // Failed without much info, there is little we can do about this.
                debug!(err=%display_error(error), "connection failed early");
                Effects::new()
            }
            IncomingConnection::Failed {
                peer_addr: _,
                peer_id: _,
                ref error,
            } => {
                // TODO: At this point, we could consider blocking peers by [`PeerID`], but this
                //       feature is not implemented yet.
                debug!(
                    err = display_error(error),
                    "connection failed after TLS setup"
                );
                Effects::new()
            }
            IncomingConnection::Loopback => {
                // Loopback connections are closed immediately, but will be marked as such by the
                // outgoing manager. We still record that it succeeded in the log, but this should
                // be the only time per component instantiation that this happens.
                info!("successful loopback connection, will be dropped");
                Effects::new()
            }
            IncomingConnection::Established {
                peer_addr,
                public_addr,
                peer_id,
                stream,
            } => {
                info!("new incoming connection established");

                // Learn the address the peer gave us.
                let dial_requests =
                    self.outgoing_manager
                        .learn_addr(public_addr, false, Instant::now());
                let mut effects = self.process_dial_requests(dial_requests);

                // Update connection symmetries.
                if self
                    .connection_symmetries
                    .entry(peer_id)
                    .or_default()
                    .add_incoming(peer_addr, Instant::now())
                {
                    effects.extend(self.connection_completed(effect_builder, peer_id));
                }

                // Now we can start the message reader.
                let boxed_span = Box::new(span.clone());
                effects.extend(
                    tasks::message_reader(
                        self.context.clone(),
                        stream,
                        self.shutdown_receiver.clone(),
                        peer_id,
                    )
                    .instrument(span)
                    .event(move |result| Event::IncomingClosed {
                        result,
                        peer_id: Box::new(peer_id),
                        peer_addr,
                        span: boxed_span,
                    }),
                );

                effects
            }
        })
    }

    fn handle_incoming_closed(
        &mut self,
        result: io::Result<()>,
        peer_id: Box<NodeId>,
        peer_addr: SocketAddr,
        span: Span,
    ) -> Effects<Event<P>> {
        span.in_scope(|| {
            // Log the outcome.
            match result {
                Ok(()) => {
                    info!("regular connection close")
                }
                Err(ref err) => {
                    warn!(err = display_error(err), "connection dropped")
                }
            }

            // Update the connection symmetries.
            self.connection_symmetries
                .entry(*peer_id)
                .or_default()
                .remove_incoming(peer_addr, Instant::now());

            Effects::new()
        })
    }

    /// Sets up an established outgoing connection.
    ///
    /// Initiates sending of the handshake as soon as the connection is established.
    fn setup_outgoing(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_addr: SocketAddr,
        peer_id: NodeId,
        transport: Transport,
    ) -> Effects<Event<P>> {
        // If we have connected to ourself, inform the outgoing manager, then drop the connection.
        if peer_id == self.context.our_id {
            let request = self
                .outgoing_manager
                .handle_dial_outcome(DialOutcome::Loopback { addr: peer_addr });
            return self.process_dial_requests(request);
        }

        // The stream is only used to receive a single handshake message and then dropped.
        let (sink, stream) = framed::<P>(
            Arc::downgrade(&self.net_metrics),
            ConnectionId::from_connection(transport.ssl(), self.context.our_id, peer_id),
            transport,
            Role::Dialer,
            self.context.chain_info.maximum_net_message_size,
        )
        .split();

        // Setup the actual connection and inform the outgoing manager and record in symmetry.
        let (sender, receiver) = mpsc::unbounded_channel();
        let connection = OutgoingConnection { peer_addr, sender };

        let mut effects = Effects::new();

        // Mark as outgoing in symmetry tracker.
        if self
            .connection_symmetries
            .entry(peer_id)
            .or_default()
            .mark_outgoing(Instant::now())
        {
            effects.extend(self.connection_completed(effect_builder, peer_id));
        }

        let request = self
            .outgoing_manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: peer_addr,
                handle: connection,
                node_id: peer_id,
            });

        // Process resulting effects.
        effects.extend(self.process_dial_requests(request));

        // Send the handshake and start the reader.
        let handshake = self
            .context
            .chain_info
            .create_handshake(self.context.public_addr);

        effects.extend(
            tasks::message_sender(
                receiver,
                sink,
                self.net_metrics.queued_messages.clone(),
                handshake,
            )
            .event(move |result| Event::OutgoingDropped {
                peer_id: Box::new(peer_id),
                peer_addr: Box::new(peer_addr),
                error: Box::new(result.err().map(Into::into)),
            }),
        );
        effects.extend(
            tasks::read_handshake(
                self.context.event_queue,
                stream,
                self.context.our_id,
                peer_id,
                peer_addr,
            )
            .ignore::<Event<P>>(),
        );

        effects
    }

    fn handle_outgoing_lost(
        &mut self,
        peer_id: NodeId,
        peer_addr: SocketAddr,
    ) -> Effects<Event<P>> {
        let requests = self
            .outgoing_manager
            .handle_connection_drop(peer_addr, Instant::now());

        self.connection_symmetries
            .entry(peer_id)
            .or_default()
            .unmark_outgoing(Instant::now());

        self.process_dial_requests(requests)
    }

    /// Gossips our public listening address, and schedules the next such gossip round.
    fn gossip_our_address(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event<P>> {
        let our_address = GossipedAddress::new(self.context.public_addr);
        effect_builder
            .announce_gossip_our_address(our_address)
            .ignore()
    }

    /// Sweeps across connection symmetry, enforcing symmetrical connections.
    fn enforce_symmetric_connections(&mut self, now: Instant) -> Effects<Event<P>> {
        let mut dial_requests = Vec::new();

        let conn_syms = mem::take(&mut self.connection_symmetries);
        self.connection_symmetries = conn_syms
            .into_iter()
            .filter_map(|(peer_id, sym)| {
                if sym.should_be_reaped(now, MAX_ASYMMETRIC_TIME) {
                    info!(%peer_id, "reaping asymmetric connection");

                    // Get the outgoing connection and block it.
                    if let Some(addr) = self.outgoing_manager.get_addr(peer_id) {
                            if let Some(req) = self.outgoing_manager.block_addr(addr, now) {
                                dial_requests.push(req);
                            }
                    } else {
                        debug!(%peer_id, "tried to reap non-existent asymmetric connection, must be incoming only");
                    }

                    None
                } else {
                    Some((peer_id, sym))
                }
            })
            .collect();

        self.process_dial_requests(dial_requests)
    }

    /// Processes a set of `DialRequest`s, updating the component and emitting needed effects.
    fn process_dial_requests<T>(&mut self, requests: T) -> Effects<Event<P>>
    where
        T: IntoIterator<Item = DialRequest<OutgoingConnection<P>>>,
    {
        let mut effects = Effects::new();

        for request in requests.into_iter() {
            trace!(%request, "processing dial request");
            match request {
                DialRequest::Dial { addr, span } => effects.extend(
                    tasks::connect_outgoing(
                        addr,
                        self.context.our_cert.clone(),
                        self.context.secret_key.clone(),
                    )
                    .instrument(span)
                    .result(
                        move |(peer_id, transport)| Event::OutgoingEstablished {
                            remote_addr: addr,
                            peer_id: Box::new(peer_id),
                            transport,
                        },
                        move |error| Event::OutgoingDialFailure {
                            peer_addr: Box::new(addr),
                            error: Box::new(error),
                        },
                    ),
                ),
                DialRequest::Disconnect { handle: _, span } => {
                    // Dropping the `handle` is enough to signal the connection to shutdown.
                    span.in_scope(|| {
                        debug!("dropping connection, as requested");
                    })
                }
            }
        }

        effects
    }

    /// Handles a received message.
    fn handle_message(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
        msg: Message<P>,
    ) -> Effects<Event<P>>
    where
        REv: From<NetworkAnnouncement<NodeId, P>>,
    {
        match msg {
            Message::Handshake {
                network_name,
                public_addr,
                protocol_version,
            } => {
                let requests = if network_name != self.context.chain_info.network_name {
                    info!(
                        our_id=%self.context.our_id,
                        %peer_id,
                        our_network=?self.context.chain_info.network_name,
                        their_network=?network_name,
                        our_protocol_version=%self.context.chain_info.protocol_version,
                        their_protocol_version=%protocol_version,
                        "dropping connection due to network name mismatch"
                    );

                    // Node is on the wrong network. As a workaround until the connection logic
                    // eliminates these cases immediately, we look up the peer's outgoing
                    // connections and block them.
                    if let Some(addr) = self.outgoing_manager.get_addr(peer_id) {
                        self.outgoing_manager.block_addr(addr, Instant::now())
                    } else {
                        Default::default()
                    }
                } else {
                    // Speed up the connection process by directly learning the peer's address.
                    self.outgoing_manager
                        .learn_addr(public_addr, false, Instant::now())
                };
                self.process_dial_requests(requests)
            }
            Message::Payload(payload) => effect_builder
                .announce_message_received(peer_id, payload)
                .ignore(),
        }
    }

    /// Emits an announcement that a connection has been completed.
    fn connection_completed(
        &self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
    ) -> Effects<Event<P>> {
        effect_builder.announce_new_peer(peer_id).ignore()
    }

    /// Returns the set of connected nodes.
    pub(crate) fn peers(&self) -> BTreeMap<NodeId, String> {
        let mut ret = BTreeMap::new();
        for node_id in self.outgoing_manager.connected_peers() {
            if let Some(connection) = self.outgoing_manager.get_route(node_id) {
                ret.insert(node_id, connection.peer_addr.to_string());
            } else {
                // This should never happen unless the state of `OutgoingManager` is corrupt.
                warn!(%node_id, "route disappeared unexpectedly")
            }
        }

        // Keeping the old interface intact, we simply override earlier with later addresses.
        for (node_id, sym) in &self.connection_symmetries {
            if let Some(addrs) = sym.incoming_addrs() {
                for addr in addrs {
                    ret.insert(*node_id, addr.to_string());
                }
            }
        }
        ret
    }

    /// Returns the node id of this network node.
    #[cfg(test)]
    pub(crate) fn node_id(&self) -> NodeId {
        self.context.our_id
    }
}

impl<REv, P> Finalize for SmallNetwork<REv, P>
where
    REv: Send + 'static,
    P: Payload,
{
    fn finalize(mut self) -> BoxFuture<'static, ()> {
        async move {
            // Close the shutdown socket, causing the server to exit.
            drop(self.shutdown_sender.take());

            // Wait for the server to exit cleanly.
            if let Some(join_handle) = self.server_join_handle.take() {
                match join_handle.await {
                    Ok(_) => debug!(our_id=%self.context.our_id, "server exited cleanly"),
                    Err(ref err) => {
                        error!(%self.context.our_id, err=display_error(err), "could not join server task cleanly")
                    }
                }
            }

            // Ensure there are no ongoing metrics updates.
            utils::wait_for_arc_drop(self.net_metrics, MAX_METRICS_DROP_ATTEMPTS, DROP_RETRY_DELAY).await;
        }
        .boxed()
    }
}

impl<REv, P> Component<REv> for SmallNetwork<REv, P>
where
    REv: ReactorEvent + From<Event<P>> + From<NetworkAnnouncement<NodeId, P>>,
    P: Payload,
{
    type Event = Event<P>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::IncomingConnection { incoming, span } => {
                self.handle_incoming_connection(effect_builder, incoming, span)
            }
            Event::IncomingMessage { peer_id, msg } => {
                self.handle_message(effect_builder, *peer_id, *msg)
            }
            Event::IncomingClosed {
                result,
                peer_id,
                peer_addr,
                span,
            } => self.handle_incoming_closed(result, peer_id, peer_addr, *span),

            Event::OutgoingEstablished {
                remote_addr,
                peer_id,
                transport,
            } => self.setup_outgoing(effect_builder, remote_addr, *peer_id, transport),
            Event::OutgoingDialFailure { peer_addr, error } => {
                let requests = self
                    .outgoing_manager
                    .handle_dial_outcome(DialOutcome::Failed {
                        addr: *peer_addr,
                        error: *error,
                        when: Instant::now(),
                    });

                self.process_dial_requests(requests)
            }
            Event::OutgoingDropped {
                peer_id,
                peer_addr,
                error: _,
            } => self.handle_outgoing_lost(*peer_id, *peer_addr),

            Event::NetworkRequest { req } => {
                match *req {
                    NetworkRequest::SendMessage {
                        dest,
                        payload,
                        responder,
                    } => {
                        // We're given a message to send out.
                        self.net_metrics.direct_message_requests.inc();
                        self.send_message(*dest, Message::Payload(*payload));
                        responder.respond(()).ignore()
                    }
                    NetworkRequest::Broadcast { payload, responder } => {
                        // We're given a message to broadcast.
                        self.net_metrics.broadcast_requests.inc();
                        self.broadcast_message(Message::Payload(*payload));
                        responder.respond(()).ignore()
                    }
                    NetworkRequest::Gossip {
                        payload,
                        count,
                        exclude,
                        responder,
                    } => {
                        // We're given a message to gossip.
                        let sent_to =
                            self.gossip_message(rng, Message::Payload(*payload), count, exclude);
                        responder.respond(sent_to).ignore()
                    }
                }
            }
            Event::NetworkInfoRequest { req } => match *req {
                NetworkInfoRequest::GetPeers { responder } => {
                    responder.respond(self.peers()).ignore()
                }
            },
            Event::PeerAddressReceived(gossiped_address) => {
                let requests = self.outgoing_manager.learn_addr(
                    gossiped_address.into(),
                    false,
                    Instant::now(),
                );
                self.process_dial_requests(requests)
            }
            Event::BlocklistAnnouncement(BlocklistAnnouncement::OffenseCommitted(peer_id)) => {
                // TODO: We do not have a proper by-node-ID blocklist, but rather only block the
                // current outgoing address of a peer.
                warn!(%peer_id, "adding peer to blocklist after transgression");

                if let Some(addr) = self.outgoing_manager.get_addr(*peer_id) {
                    let requests = self.outgoing_manager.block_addr(addr, Instant::now());
                    self.process_dial_requests(requests)
                } else {
                    // Peer got away with it, no longer an outgoing connection.
                    Effects::new()
                }
            }

            Event::GossipOurAddress => {
                let mut effects = self.gossip_our_address(effect_builder);
                effects.extend(
                    effect_builder
                        .set_timeout(self.cfg.gossip_interval)
                        .event(|_| Event::GossipOurAddress),
                );
                effects
            }

            Event::SweepSymmetries => {
                let now = Instant::now();

                let mut effects = self.enforce_symmetric_connections(now);

                effects.extend(
                    effect_builder
                        .set_timeout(SYMMETRY_SWEEP_INTERVAL)
                        .event(|_| Event::SweepSymmetries),
                );

                effects
            }
            Event::SweepOutgoing => {
                let now = Instant::now();
                let requests = self.outgoing_manager.perform_housekeeping(now);
                let mut effects = self.process_dial_requests(requests);

                effects.extend(
                    effect_builder
                        .set_timeout(OUTGOING_MANAGER_SWEEP_INTERVAL)
                        .event(|_| Event::SweepOutgoing),
                );

                effects
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum SmallNetworkIdentityError {
    #[error("could not generate TLS certificate: {0}")]
    CouldNotGenerateTlsCertificate(OpenSslErrorStack),
    #[error(transparent)]
    ValidationError(#[from] ValidationError),
}

/// An ephemeral [PKey<Private>] and [TlsCert] that identifies this node
#[derive(DataSize, Debug, Clone)]
pub struct SmallNetworkIdentity {
    secret_key: Arc<PKey<Private>>,
    tls_certificate: Arc<TlsCert>,
}

impl SmallNetworkIdentity {
    pub fn new() -> result::Result<Self, SmallNetworkIdentityError> {
        let (not_yet_validated_x509_cert, secret_key) = tls::generate_node_cert()
            .map_err(SmallNetworkIdentityError::CouldNotGenerateTlsCertificate)?;
        let tls_certificate = tls::validate_cert(not_yet_validated_x509_cert)?;
        Ok(SmallNetworkIdentity {
            secret_key: Arc::new(secret_key),
            tls_certificate: Arc::new(tls_certificate),
        })
    }
}

impl<REv, P> From<&SmallNetwork<REv, P>> for SmallNetworkIdentity
where
    P: Payload,
{
    fn from(small_network: &SmallNetwork<REv, P>) -> Self {
        SmallNetworkIdentity {
            secret_key: small_network.context.secret_key.clone(),
            tls_certificate: small_network.context.our_cert.clone(),
        }
    }
}

impl From<&SmallNetworkIdentity> for NodeId {
    fn from(small_network_identity: &SmallNetworkIdentity) -> Self {
        NodeId::from(
            small_network_identity
                .tls_certificate
                .public_key_fingerprint(),
        )
    }
}

/// Transport type alias for base encrypted connections.
type Transport = SslStream<TcpStream>;

/// A framed transport for `Message`s.
pub type FramedTransport<P> = SymmetricallyFramed<
    Framed<Transport, LengthDelimitedCodec>,
    Message<P>,
    CountingFormat<MessagePackFormat>,
>;

/// Constructs a new framed transport on a stream.
fn framed<P>(
    metrics: Weak<NetworkingMetrics>,
    connection_id: ConnectionId,
    stream: Transport,
    role: Role,
    maximum_net_message_size: u32,
) -> FramedTransport<P>
where
    for<'de> P: Serialize + Deserialize<'de>,
    for<'de> Message<P>: Serialize + Deserialize<'de>,
{
    let length_delimited = Framed::new(
        stream,
        LengthDelimitedCodec::builder()
            .max_frame_length(maximum_net_message_size as usize)
            .new_codec(),
    );

    SymmetricallyFramed::new(
        length_delimited,
        CountingFormat::new(metrics, connection_id, role, MessagePackFormat),
    )
}

impl<R, P> Debug for SmallNetwork<R, P>
where
    P: Payload,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // We output only the most important fields of the component, as it gets unwieldy quite fast
        // otherwise.
        f.debug_struct("SmallNetwork")
            .field("our_id", &self.context.our_id)
            .field("public_addr", &self.context.public_addr)
            .finish()
    }
}
