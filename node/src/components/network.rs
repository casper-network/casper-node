//! Fully connected overlay network
//!
//! The *network component* is an overlay network where each node participating is attempting to
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

mod bincode_format;
pub(crate) mod blocklist;
mod chain_info;
mod config;
mod counting_format;
mod error;
mod event;
mod gossiped_address;
mod identity;
mod insights;
mod limiter;
mod message;
mod message_pack_format;
mod metrics;
mod outgoing;
mod symmetry;
pub(crate) mod tasks;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    io,
    net::{SocketAddr, TcpListener},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use prometheus::Registry;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{self, UnboundedSender},
        watch,
    },
    task::JoinHandle,
};
use tokio_openssl::SslStream;
use tokio_util::codec::LengthDelimitedCodec;
use tracing::{debug, error, info, trace, warn, Instrument, Span};

use casper_types::{EraId, PublicKey, SecretKey};

pub(crate) use self::{
    bincode_format::BincodeFormat,
    config::{Config, IdentityConfig},
    error::Error,
    event::Event,
    gossiped_address::GossipedAddress,
    identity::Identity,
    insights::NetworkInsights,
    message::{EstimatorWeights, FromIncoming, Message, MessageKind, Payload},
};
use self::{
    blocklist::BlocklistJustification,
    chain_info::ChainInfo,
    counting_format::{ConnectionId, CountingFormat, Role},
    error::{ConnectionError, Result},
    event::{IncomingConnection, OutgoingConnection},
    limiter::Limiter,
    message::NodeKeyPair,
    metrics::Metrics,
    outgoing::{DialOutcome, DialRequest, OutgoingConfig, OutgoingManager},
    symmetry::ConnectionSymmetry,
    tasks::{MessageQueueItem, NetworkContext},
};
use crate::{
    components::{Component, ComponentStatus, InitializedComponent},
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{BeginGossipRequest, NetworkInfoRequest, NetworkRequest, StorageRequest},
        AutoClosingResponder, EffectBuilder, EffectExt, Effects, GossipTarget,
    },
    reactor::{Finalize, ReactorEvent},
    tls,
    types::{NodeId, ValidatorMatrix},
    utils::{self, display_error, Source},
    NodeRng,
};

const MAX_METRICS_DROP_ATTEMPTS: usize = 25;
const DROP_RETRY_DELAY: Duration = Duration::from_millis(100);

/// How often to keep attempting to reconnect to a node before giving up. Note that reconnection
/// delays increase exponentially!
const RECONNECTION_ATTEMPTS: u8 = 8;

/// Basic reconnection timeout.
///
/// The first reconnection attempt will be made after 2x this timeout.
const BASE_RECONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

/// Interval during which to perform outgoing manager housekeeping.
const OUTGOING_MANAGER_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, DataSize, Debug)]
pub(crate) struct OutgoingHandle<P> {
    #[data_size(skip)] // Unfortunately, there is no way to inspect an `UnboundedSender`.
    sender: UnboundedSender<MessageQueueItem<P>>,
    peer_addr: SocketAddr,
}

impl<P> Display for OutgoingHandle<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "outgoing handle to {}", self.peer_addr)
    }
}

#[derive(DataSize)]
pub(crate) struct Network<REv, P>
where
    REv: 'static,
    P: Payload,
{
    /// Initial configuration values.
    cfg: Config,
    /// Read-only networking information shared across tasks.
    context: Arc<NetworkContext<REv>>,

    /// Outgoing connections manager.
    outgoing_manager: OutgoingManager<OutgoingHandle<P>, ConnectionError>,
    /// Tracks whether a connection is symmetric or not.
    connection_symmetries: HashMap<NodeId, ConnectionSymmetry>,

    /// Tracks nodes that have announced themselves as nodes that are syncing.
    syncing_nodes: HashSet<NodeId>,

    channel_management: Option<ChannelManagement>,

    /// Networking metrics.
    #[data_size(skip)]
    net_metrics: Arc<Metrics>,

    /// The outgoing bandwidth limiter.
    #[data_size(skip)]
    outgoing_limiter: Limiter,

    /// The limiter for incoming resource usage.
    ///
    /// This is not incoming bandwidth but an independent resource estimate.
    #[data_size(skip)]
    incoming_limiter: Limiter,

    /// The era that is considered the active era by the network component.
    active_era: EraId,

    /// The status of this component.
    status: ComponentStatus,
}

#[derive(DataSize)]
struct ChannelManagement {
    /// Channel signaling a shutdown of the network.
    // Note: This channel is closed when `Network` is dropped, signalling the receivers that
    // they should cease operation.
    #[data_size(skip)]
    shutdown_sender: Option<watch::Sender<()>>,
    /// Join handle for the server thread.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,

    /// Channel signaling a shutdown of the incoming connections.
    // Note: This channel is closed when we finished syncing, so the `Network` can close all
    // connections. When they are re-established, the proper value of the now updated `is_syncing`
    // flag will be exchanged on handshake.
    #[data_size(skip)]
    close_incoming_sender: Option<watch::Sender<()>>,
    /// Handle used by the `message_reader` task to receive a notification that incoming
    /// connections should be closed.
    #[data_size(skip)]
    close_incoming_receiver: watch::Receiver<()>,
}

impl<REv, P> Network<REv, P>
where
    P: Payload + 'static,
    REv: ReactorEvent
        + From<Event<P>>
        + FromIncoming<P>
        + From<StorageRequest>
        + From<NetworkRequest<P>>
        + From<PeerBehaviorAnnouncement>,
{
    /// Creates a new network component instance.
    #[allow(clippy::type_complexity)]
    pub(crate) fn new<C: Into<ChainInfo>>(
        cfg: Config,
        our_identity: Identity,
        node_key_pair: Option<(Arc<SecretKey>, PublicKey)>,
        registry: &Registry,
        chain_info_source: C,
        validator_matrix: ValidatorMatrix,
    ) -> Result<Network<REv, P>> {
        let net_metrics = Arc::new(Metrics::new(registry)?);

        let outgoing_limiter = Limiter::new(
            cfg.max_outgoing_byte_rate_non_validators,
            net_metrics.accumulated_outgoing_limiter_delay.clone(),
            validator_matrix.clone(),
        );

        let incoming_limiter = Limiter::new(
            cfg.max_incoming_message_rate_non_validators,
            net_metrics.accumulated_incoming_limiter_delay.clone(),
            validator_matrix,
        );

        let outgoing_manager = OutgoingManager::with_metrics(
            OutgoingConfig {
                retry_attempts: RECONNECTION_ATTEMPTS,
                base_timeout: BASE_RECONNECTION_TIMEOUT,
                unblock_after: cfg.blocklist_retain_duration.into(),
                sweep_timeout: cfg.max_addr_pending_time.into(),
            },
            net_metrics.create_outgoing_metrics(),
        );

        let context = Arc::new(NetworkContext::new(
            cfg.clone(),
            our_identity,
            node_key_pair.map(NodeKeyPair::new),
            chain_info_source.into(),
            &net_metrics,
        ));

        let component = Network {
            cfg,
            context,
            outgoing_manager,
            connection_symmetries: HashMap::new(),
            syncing_nodes: HashSet::new(),
            channel_management: None,
            net_metrics,
            outgoing_limiter,
            incoming_limiter,
            // We start with an empty set of validators for era 0 and expect to be updated.
            active_era: EraId::new(0),
            status: ComponentStatus::Uninitialized,
        };

        Ok(component)
    }

    fn initialize(&mut self, effect_builder: EffectBuilder<REv>) -> Result<Effects<Event<P>>> {
        let mut known_addresses = HashSet::new();
        for address in &self.cfg.known_addresses {
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
            return Err(Error::EmptyKnownHosts);
        }

        let mut public_addr =
            utils::resolve_address(&self.cfg.public_address).map_err(Error::ResolveAddr)?;

        // We can now create a listener.
        let bind_address =
            utils::resolve_address(&self.cfg.bind_address).map_err(Error::ResolveAddr)?;
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

        Arc::get_mut(&mut self.context)
            .expect("should be no other pointers")
            .initialize(public_addr, effect_builder.into_inner());

        let protocol_version = self.context.chain_info().protocol_version;
        // Run the server task.
        // We spawn it ourselves instead of through an effect to get a hold of the join handle,
        // which we need to shutdown cleanly later on.
        info!(%local_addr, %public_addr, %protocol_version, "starting server background task");

        let (server_shutdown_sender, server_shutdown_receiver) = watch::channel(());
        let (close_incoming_sender, close_incoming_receiver) = watch::channel(());

        let context = self.context.clone();
        let server_join_handle = tokio::spawn(
            tasks::server(
                context,
                tokio::net::TcpListener::from_std(listener).map_err(Error::ListenerConversion)?,
                server_shutdown_receiver,
            )
            .in_current_span(),
        );

        let channel_management = ChannelManagement {
            shutdown_sender: Some(server_shutdown_sender),
            server_join_handle: Some(server_join_handle),
            close_incoming_sender: Some(close_incoming_sender),
            close_incoming_receiver,
        };

        self.channel_management = Some(channel_management);

        // Learn all known addresses and mark them as unforgettable.
        let now = Instant::now();
        let dial_requests: Vec<_> = known_addresses
            .into_iter()
            .filter_map(|addr| self.outgoing_manager.learn_addr(addr, true, now))
            .collect();

        let mut effects = self.process_dial_requests(dial_requests);

        // Start broadcasting our public listening address.
        effects.extend(
            effect_builder
                .set_timeout(self.cfg.initial_gossip_delay.into())
                .event(|_| Event::GossipOurAddress),
        );

        // Start regular housekeeping of the outgoing connections.
        effects.extend(
            effect_builder
                .set_timeout(OUTGOING_MANAGER_SWEEP_INTERVAL)
                .event(|_| Event::SweepOutgoing),
        );

        self.status = ComponentStatus::Initialized;
        Ok(effects)
    }

    /// Should only be called after component has been initialized.
    fn channel_management(&self) -> &ChannelManagement {
        self.channel_management
            .as_ref()
            .expect("component not initialized properly")
    }

    /// Queues a message to be sent to validator nodes in the given era.
    fn broadcast_message_to_validators(&self, msg: Arc<Message<P>>, era_id: EraId) {
        self.net_metrics.broadcast_requests.inc();
        for peer_id in self.outgoing_manager.connected_peers() {
            if self.outgoing_limiter.is_validator_in_era(era_id, &peer_id) {
                self.send_message(peer_id, msg.clone(), None)
            }
        }
    }

    /// Queues a message to `count` random nodes on the network.
    fn gossip_message(
        &self,
        rng: &mut NodeRng,
        msg: Arc<Message<P>>,
        gossip_target: GossipTarget,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let peer_ids = self
            .outgoing_manager
            .connected_peers()
            .filter(|peer_id| {
                if exclude.contains(peer_id) {
                    return false;
                }

                match gossip_target {
                    GossipTarget::All => true,
                    GossipTarget::NonValidators(era_id) => {
                        // If the peer isn't a validator, include it.
                        !self.outgoing_limiter.is_validator_in_era(era_id, peer_id)
                    }
                }
            })
            .choose_multiple(rng, count);

        if peer_ids.len() != count {
            // TODO - set this to `warn!` once we are normally testing with networks large enough to
            //        make it a meaningful and infrequent log message.
            trace!(
                our_id=%self.context.our_id(),
                wanted = count,
                selected = peer_ids.len(),
                "could not select enough random nodes for gossiping, not enough non-excluded \
                outgoing connections"
            );
        }

        for &peer_id in &peer_ids {
            self.send_message(peer_id, msg.clone(), None);
        }

        peer_ids.into_iter().collect()
    }

    /// Queues a message to be sent to a specific node.
    fn send_message(
        &self,
        dest: NodeId,
        msg: Arc<Message<P>>,
        opt_responder: Option<AutoClosingResponder<()>>,
    ) {
        // Try to send the message.
        if let Some(connection) = self.outgoing_manager.get_route(dest) {
            if msg.payload_is_unsafe_for_syncing_nodes() && self.syncing_nodes.contains(&dest) {
                // We should never attempt to send an unsafe message to a peer that we know is still
                // syncing. Since "unsafe" does usually not mean immediately catastrophic, we
                // attempt to carry on, but warn loudly.
                error!(kind=%msg.classify(), node_id=%dest, "sending unsafe message to syncing node");
            }

            if let Err(msg) = connection.sender.send((msg, opt_responder)) {
                // We lost the connection, but that fact has not reached us yet.
                warn!(our_id=%self.context.our_id(), %dest, ?msg, "dropped outgoing message, lost connection");
            } else {
                self.net_metrics.queued_messages.inc();
            }
        } else {
            // We are not connected, so the reconnection is likely already in progress.
            debug!(our_id=%self.context.our_id(), %dest, ?msg, "dropped outgoing message, no connection");
        }
    }

    fn handle_incoming_connection(
        &mut self,
        incoming: Box<IncomingConnection<P>>,
        span: Span,
    ) -> Effects<Event<P>> {
        span.clone().in_scope(|| match *incoming {
            IncomingConnection::FailedEarly {
                peer_addr: _,
                ref error,
            } => {
                // Failed without much info, there is little we can do about this.
                debug!(err=%display_error(error), "incoming connection failed early");
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
                    "incoming connection failed after TLS setup"
                );
                Effects::new()
            }
            IncomingConnection::Loopback => {
                // Loopback connections are closed immediately, but will be marked as such by the
                // outgoing manager. We still record that it succeeded in the log, but this should
                // be the only time per component instantiation that this happens.
                info!("successful incoming loopback connection, will be dropped");
                Effects::new()
            }
            IncomingConnection::Established {
                peer_addr,
                public_addr,
                peer_id,
                peer_consensus_public_key,
                stream,
            } => {
                if self.cfg.max_incoming_peer_connections != 0 {
                    if let Some(symmetries) = self.connection_symmetries.get(&peer_id) {
                        let incoming_count = symmetries
                            .incoming_addrs()
                            .map(|addrs| addrs.len())
                            .unwrap_or_default();

                        if incoming_count >= self.cfg.max_incoming_peer_connections as usize {
                            info!(%public_addr,
                                  %peer_id,
                                  count=incoming_count,
                                  limit=self.cfg.max_incoming_peer_connections,
                                  "rejecting new incoming connection, limit for peer exceeded"
                            );
                            return Effects::new();
                        }
                    }
                }

                info!(%public_addr, "new incoming connection established");

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
                    self.connection_completed(peer_id);

                    // We should NOT update the syncing set when we receive an incoming connection,
                    // because the `message_sender` which is handling the corresponding outgoing
                    // connection will not receive the update of the syncing state of the remote
                    // peer.
                    //
                    // Such desync may cause the node to try to send "unsafe" requests to the
                    // syncing node, because the outgoing connection may outlive the
                    // incoming one, i.e. it may take some time to drop "our" outgoing
                    // connection after a peer has closed the corresponding incoming connection.
                }

                // Now we can start the message reader.
                let boxed_span = Box::new(span.clone());
                effects.extend(
                    tasks::message_reader(
                        self.context.clone(),
                        stream,
                        self.incoming_limiter
                            .create_handle(peer_id, peer_consensus_public_key),
                        self.channel_management().close_incoming_receiver.clone(),
                        peer_id,
                        span.clone(),
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
                    info!("regular connection closing")
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

    /// Determines whether an outgoing peer should be blocked based on the connection error.
    fn is_blockable_offense_for_outgoing(
        &self,
        error: &ConnectionError,
    ) -> Option<BlocklistJustification> {
        match error {
            // Potentially transient failures.
            //
            // Note that incompatible versions need to be considered transient, since they occur
            // during regular upgrades.
            ConnectionError::TlsInitialization(_)
            | ConnectionError::TcpConnection(_)
            | ConnectionError::TcpNoDelay(_)
            | ConnectionError::TlsHandshake(_)
            | ConnectionError::HandshakeSend(_)
            | ConnectionError::HandshakeRecv(_)
            | ConnectionError::IncompatibleVersion(_) => None,

            // These errors are potential bugs on our side.
            ConnectionError::HandshakeSenderCrashed(_)
            | ConnectionError::FailedToReuniteHandshakeSinkAndStream
            | ConnectionError::CouldNotEncodeOurHandshake(_) => None,

            // These could be candidates for blocking, but for now we decided not to.
            ConnectionError::NoPeerCertificate
            | ConnectionError::PeerCertificateInvalid(_)
            | ConnectionError::DidNotSendHandshake
            | ConnectionError::InvalidRemoteHandshakeMessage(_)
            | ConnectionError::InvalidConsensusCertificate(_) => None,

            // Definitely something we want to avoid.
            ConnectionError::WrongNetwork(peer_network_name) => {
                Some(BlocklistJustification::WrongNetwork {
                    peer_network_name: peer_network_name.clone(),
                })
            }
            ConnectionError::WrongChainspecHash(peer_chainspec_hash) => {
                Some(BlocklistJustification::WrongChainspecHash {
                    peer_chainspec_hash: *peer_chainspec_hash,
                })
            }
            ConnectionError::MissingChainspecHash => {
                Some(BlocklistJustification::MissingChainspecHash)
            }
        }
    }

    /// Sets up an established outgoing connection.
    ///
    /// Initiates sending of the handshake as soon as the connection is established.
    #[allow(clippy::redundant_clone)]
    fn handle_outgoing_connection(
        &mut self,
        outgoing: OutgoingConnection<P>,
        span: Span,
    ) -> Effects<Event<P>> {
        let now = Instant::now();
        span.clone().in_scope(|| match outgoing {
            OutgoingConnection::FailedEarly { peer_addr, error }
            | OutgoingConnection::Failed {
                peer_addr,
                peer_id: _,
                error,
            } => {
                debug!(err=%display_error(&error), "outgoing connection failed");
                // We perform blocking first, to not trigger a reconnection before blocking.
                let mut requests = Vec::new();

                if let Some(justification) = self.is_blockable_offense_for_outgoing(&error) {
                    requests.extend(
                        self.outgoing_manager
                            .block_addr(peer_addr, now, justification)
                            .into_iter(),
                    );
                }

                // Now we can proceed with the regular updates.
                requests.extend(
                    self.outgoing_manager
                        .handle_dial_outcome(DialOutcome::Failed {
                            addr: peer_addr,
                            error,
                            when: now,
                        })
                        .into_iter(),
                );

                self.process_dial_requests(requests)
            }
            OutgoingConnection::Loopback { peer_addr } => {
                // Loopback connections are marked, but closed.
                info!("successful outgoing loopback connection, will be dropped");
                let request = self
                    .outgoing_manager
                    .handle_dial_outcome(DialOutcome::Loopback { addr: peer_addr });
                self.process_dial_requests(request)
            }
            OutgoingConnection::Established {
                peer_addr,
                peer_id,
                peer_consensus_public_key,
                sink,
                is_syncing,
            } => {
                info!("new outgoing connection established");

                let (sender, receiver) = mpsc::unbounded_channel();
                let handle = OutgoingHandle { sender, peer_addr };

                let request = self
                    .outgoing_manager
                    .handle_dial_outcome(DialOutcome::Successful {
                        addr: peer_addr,
                        handle,
                        node_id: peer_id,
                    });

                let mut effects = self.process_dial_requests(request);

                // Update connection symmetries.
                if self
                    .connection_symmetries
                    .entry(peer_id)
                    .or_default()
                    .mark_outgoing(now)
                {
                    self.connection_completed(peer_id);
                    self.update_syncing_nodes_set(peer_id, is_syncing);
                }

                effects.extend(
                    tasks::message_sender(
                        receiver,
                        sink,
                        self.outgoing_limiter
                            .create_handle(peer_id, peer_consensus_public_key),
                        self.net_metrics.queued_messages.clone(),
                    )
                    .instrument(span)
                    .event(move |_| Event::OutgoingDropped {
                        peer_id: Box::new(peer_id),
                        peer_addr,
                    }),
                );

                effects
            }
        })
    }

    fn handle_outgoing_dropped(
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

        self.outgoing_limiter.remove_connected_validator(&peer_id);

        self.process_dial_requests(requests)
    }

    /// Processes a set of `DialRequest`s, updating the component and emitting needed effects.
    fn process_dial_requests<T>(&mut self, requests: T) -> Effects<Event<P>>
    where
        T: IntoIterator<Item = DialRequest<OutgoingHandle<P>>>,
    {
        let mut effects = Effects::new();

        for request in requests.into_iter() {
            trace!(%request, "processing dial request");
            match request {
                DialRequest::Dial { addr, span } => effects.extend(
                    tasks::connect_outgoing(self.context.clone(), addr)
                        .instrument(span.clone())
                        .event(|outgoing| Event::OutgoingConnection {
                            outgoing: Box::new(outgoing),
                            span,
                        }),
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
    fn handle_incoming_message(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
        msg: Message<P>,
        span: Span,
    ) -> Effects<Event<P>>
    where
        REv: FromIncoming<P> + From<PeerBehaviorAnnouncement>,
    {
        span.in_scope(|| match msg {
            Message::Handshake { .. } => {
                // We should never receive a handshake message on an established connection. Simply
                // discard it. This may be too lenient, so we may consider simply dropping the
                // connection in the future instead.
                warn!("received unexpected handshake");
                Effects::new()
            }
            Message::Payload(payload) => {
                effect_builder.announce_incoming(peer_id, payload).ignore()
            }
        })
    }

    /// Emits an announcement that a connection has been completed.
    fn connection_completed(&self, peer_id: NodeId) {
        trace!(num_peers = self.peers().len(), new_peer=%peer_id, "connection complete");
        self.net_metrics.peers.set(self.peers().len() as i64);
    }

    /// Updates a set of known joining nodes.
    /// If we've just connected to a non-joining node that peer will be removed from the set.
    fn update_syncing_nodes_set(&mut self, peer_id: NodeId, is_syncing: bool) {
        // Update set of syncing peers.
        if is_syncing {
            debug!(%peer_id, "is syncing");
            self.syncing_nodes.insert(peer_id);
        } else {
            debug!(%peer_id, "is no longer syncing");
            self.syncing_nodes.remove(&peer_id);
        }
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

        for (node_id, sym) in &self.connection_symmetries {
            if let Some(addrs) = sym.incoming_addrs() {
                for addr in addrs {
                    ret.entry(*node_id).or_insert_with(|| addr.to_string());
                }
            }
        }

        ret
    }

    pub(crate) fn fully_connected_peers_random(
        &self,
        rng: &mut NodeRng,
        count: usize,
    ) -> Vec<NodeId> {
        self.connection_symmetries
            .iter()
            .filter_map(|(node_id, sym)| {
                matches!(sym, ConnectionSymmetry::Symmetric { .. }).then(|| *node_id)
            })
            .choose_multiple(rng, count)
    }

    pub(crate) fn has_sufficient_fully_connected_peers(&self) -> bool {
        self.connection_symmetries
            .iter()
            .filter(|(_node_id, sym)| matches!(sym, ConnectionSymmetry::Symmetric { .. }))
            .count()
            >= self.cfg.min_peers_for_initialization as usize
    }

    #[cfg(test)]
    /// Returns the node id of this network node.
    pub(crate) fn node_id(&self) -> NodeId {
        self.context.our_id()
    }
}

impl<REv, P> Finalize for Network<REv, P>
where
    REv: Send + 'static,
    P: Payload,
{
    fn finalize(mut self) -> BoxFuture<'static, ()> {
        async move {
            if let Some(mut channel_management) = self.channel_management.take() {
                // Close the shutdown socket, causing the server to exit.
                drop(channel_management.shutdown_sender.take());
                drop(channel_management.close_incoming_sender.take());

                // Wait for the server to exit cleanly.
                if let Some(join_handle) = channel_management.server_join_handle.take() {
                    match join_handle.await {
                        Ok(_) => debug!(our_id=%self.context.our_id(), "server exited cleanly"),
                        Err(ref err) => {
                            error!(
                                our_id=%self.context.our_id(),
                                err=display_error(err),
                                "could not join server task cleanly"
                            )
                        }
                    }
                }
            }

            // Ensure there are no ongoing metrics updates.
            utils::wait_for_arc_drop(
                self.net_metrics,
                MAX_METRICS_DROP_ATTEMPTS,
                DROP_RETRY_DELAY,
            )
            .await;
        }
        .boxed()
    }
}

impl<REv, P> Component<REv> for Network<REv, P>
where
    REv: ReactorEvent
        + From<Event<P>>
        + From<BeginGossipRequest<GossipedAddress>>
        + FromIncoming<P>
        + From<StorageRequest>
        + From<NetworkRequest<P>>
        + From<PeerBehaviorAnnouncement>,
    P: Payload,
{
    type Event = Event<P>;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match (self.status.clone(), event) {
            (ComponentStatus::Fatal(msg), _) => {
                error!(
                    msg,
                    "should not handle this event when network component has fatal error"
                );
                Effects::new()
            }
            (ComponentStatus::Uninitialized, Event::Initialize) => {
                match self.initialize(effect_builder) {
                    Ok(effects) => effects,
                    Err(error) => {
                        error!(%error, "failed to initialize network component");
                        self.status = ComponentStatus::Fatal(error.to_string());
                        Effects::new()
                    }
                }
            }
            (ComponentStatus::Uninitialized, _) => {
                warn!("should not handle this event when network component is uninitialized");
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::Initialize) => {
                // noop
                Effects::new()
            }
            (ComponentStatus::Initialized, Event::IncomingConnection { incoming, span }) => {
                self.handle_incoming_connection(incoming, span)
            }
            (ComponentStatus::Initialized, Event::IncomingMessage { peer_id, msg, span }) => {
                self.handle_incoming_message(effect_builder, *peer_id, *msg, span)
            }
            (
                ComponentStatus::Initialized,
                Event::IncomingClosed {
                    result,
                    peer_id,
                    peer_addr,
                    span,
                },
            ) => self.handle_incoming_closed(result, peer_id, peer_addr, *span),

            (ComponentStatus::Initialized, Event::OutgoingConnection { outgoing, span }) => {
                self.handle_outgoing_connection(*outgoing, span)
            }

            (ComponentStatus::Initialized, Event::OutgoingDropped { peer_id, peer_addr }) => {
                self.handle_outgoing_dropped(*peer_id, peer_addr)
            }

            (ComponentStatus::Initialized, Event::NetworkRequest { req }) => {
                match *req {
                    NetworkRequest::SendMessage {
                        dest,
                        payload,
                        respond_after_queueing,
                        auto_closing_responder,
                    } => {
                        // We're given a message to send. Pass on the responder so that confirmation
                        // can later be given once the message has actually been buffered.
                        self.net_metrics.direct_message_requests.inc();

                        if respond_after_queueing {
                            self.send_message(*dest, Arc::new(Message::Payload(*payload)), None);
                            auto_closing_responder.respond(()).ignore()
                        } else {
                            self.send_message(
                                *dest,
                                Arc::new(Message::Payload(*payload)),
                                Some(auto_closing_responder),
                            );
                            Effects::new()
                        }
                    }
                    NetworkRequest::ValidatorBroadcast {
                        payload,
                        era_id,
                        auto_closing_responder,
                    } => {
                        // We're given a message to broadcast.
                        self.broadcast_message_to_validators(
                            Arc::new(Message::Payload(*payload)),
                            era_id,
                        );
                        auto_closing_responder.respond(()).ignore()
                    }
                    NetworkRequest::Gossip {
                        payload,
                        gossip_target,
                        count,
                        exclude,
                        auto_closing_responder,
                    } => {
                        // We're given a message to gossip.
                        let sent_to = self.gossip_message(
                            rng,
                            Arc::new(Message::Payload(*payload)),
                            gossip_target,
                            count,
                            exclude,
                        );
                        auto_closing_responder.respond(sent_to).ignore()
                    }
                }
            }
            (ComponentStatus::Initialized, Event::NetworkInfoRequest { req }) => match *req {
                NetworkInfoRequest::Peers { responder } => responder.respond(self.peers()).ignore(),
                NetworkInfoRequest::FullyConnectedPeers { count, responder } => responder
                    .respond(self.fully_connected_peers_random(rng, count))
                    .ignore(),
                NetworkInfoRequest::Insight { responder } => responder
                    .respond(NetworkInsights::collect_from_component(self))
                    .ignore(),
            },
            (ComponentStatus::Initialized, Event::PeerAddressReceived(gossiped_address)) => {
                let requests = self.outgoing_manager.learn_addr(
                    gossiped_address.into(),
                    false,
                    Instant::now(),
                );
                self.process_dial_requests(requests)
            }
            (
                ComponentStatus::Initialized,
                Event::BlocklistAnnouncement(PeerBehaviorAnnouncement::OffenseCommitted {
                    offender,
                    justification,
                }),
            ) => {
                // TODO: We do not have a proper by-node-ID blocklist, but rather only block the
                // current outgoing address of a peer.
                info!(%offender, %justification, "adding peer to blocklist after transgression");

                if let Some(addr) = self.outgoing_manager.get_addr(*offender) {
                    let requests =
                        self.outgoing_manager
                            .block_addr(addr, Instant::now(), *justification);
                    self.process_dial_requests(requests)
                } else {
                    // Peer got away with it, no longer an outgoing connection.
                    Effects::new()
                }
            }
            (ComponentStatus::Initialized, Event::GossipOurAddress) => {
                let our_address = GossipedAddress::new(
                    self.context
                        .public_addr()
                        .expect("component not initialized properly"),
                );

                let mut effects = effect_builder
                    .begin_gossip(our_address, Source::Ourself)
                    .ignore();
                effects.extend(
                    effect_builder
                        .set_timeout(self.cfg.gossip_interval.into())
                        .event(|_| Event::GossipOurAddress),
                );
                effects
            }
            (ComponentStatus::Initialized, Event::SweepOutgoing) => {
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

impl<REv, P> InitializedComponent<REv> for Network<REv, P>
where
    REv: ReactorEvent
        + From<Event<P>>
        + From<BeginGossipRequest<GossipedAddress>>
        + FromIncoming<P>
        + From<StorageRequest>
        + From<NetworkRequest<P>>
        + From<PeerBehaviorAnnouncement>,
    P: Payload,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }

    fn name(&self) -> &str {
        "network"
    }
}

/// Transport type alias for base encrypted connections.
type Transport = SslStream<TcpStream>;

/// A framed transport for `Message`s.
pub(crate) type FullTransport<P> = tokio_serde::Framed<
    FramedTransport,
    Message<P>,
    Arc<Message<P>>,
    CountingFormat<BincodeFormat>,
>;

pub(crate) type FramedTransport = tokio_util::codec::Framed<Transport, LengthDelimitedCodec>;

/// Constructs a new full transport on a stream.
///
/// A full transport contains the framing as well as the encoding scheme used to send messages.
fn full_transport<P>(
    metrics: Weak<Metrics>,
    connection_id: ConnectionId,
    framed: FramedTransport,
    role: Role,
) -> FullTransport<P>
where
    for<'de> P: Serialize + Deserialize<'de>,
    for<'de> Message<P>: Serialize + Deserialize<'de>,
{
    tokio_serde::Framed::new(
        framed,
        CountingFormat::new(metrics, connection_id, role, BincodeFormat::default()),
    )
}

/// Constructs a framed transport.
fn framed_transport(transport: Transport, maximum_net_message_size: u32) -> FramedTransport {
    tokio_util::codec::Framed::new(
        transport,
        LengthDelimitedCodec::builder()
            .max_frame_length(maximum_net_message_size as usize)
            .new_codec(),
    )
}

impl<R, P> Debug for Network<R, P>
where
    P: Payload,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // We output only the most important fields of the component, as it gets unwieldy quite fast
        // otherwise.
        f.debug_struct("Network")
            .field("our_id", &self.context.our_id())
            .field("status", &self.status)
            .field("public_addr", &self.context.public_addr())
            .finish()
    }
}
