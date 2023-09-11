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

pub(crate) mod blocklist;
mod chain_info;
mod config;
mod connection_id;
mod error;
mod event;
mod gossiped_address;
mod handshake;
mod health;
mod identity;
mod insights;
mod message;
mod metrics;
mod outgoing;
mod symmetry;
pub(crate) mod tasks;
#[cfg(test)]
mod tests;
mod transport;

use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    fs::OpenOptions,
    marker::PhantomData,
    net::{SocketAddr, TcpListener},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::{Duration, Instant},
};

use bincode::Options;
use bytes::Bytes;
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use itertools::Itertools;

use juliet::rpc::{JulietRpcClient, JulietRpcServer, RequestGuard, RpcBuilder};
use prometheus::Registry;
use rand::{
    seq::{IteratorRandom, SliceRandom},
    Rng,
};
use serde::Serialize;
use strum::EnumCount;
use tokio::{
    io::{ReadHalf, WriteHalf},
    net::TcpStream,
    task::JoinHandle,
};
use tokio_openssl::SslStream;
use tracing::{debug, error, info, trace, warn, Instrument, Span};

use casper_types::{EraId, PublicKey, SecretKey};

use self::{
    blocklist::BlocklistJustification,
    chain_info::ChainInfo,
    error::{ConnectionError, MessageReceiverError},
    event::{IncomingConnection, OutgoingConnection},
    health::{HealthConfig, TaggedTimestamp},
    message::NodeKeyPair,
    metrics::Metrics,
    outgoing::{DialOutcome, DialRequest, OutgoingConfig, OutgoingManager},
    symmetry::ConnectionSymmetry,
    tasks::NetworkContext,
};
pub(crate) use self::{
    config::Config,
    error::Error,
    event::Event,
    gossiped_address::GossipedAddress,
    identity::Identity,
    insights::NetworkInsights,
    message::{
        generate_largest_serialized_message, Channel, FromIncoming, Message, MessageKind, Payload,
    },
    transport::Ticket,
};
use crate::{
    components::{gossiper::GossipItem, Component, ComponentState, InitializedComponent},
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{BeginGossipRequest, NetworkInfoRequest, NetworkRequest, StorageRequest},
        AutoClosingResponder, EffectBuilder, EffectExt, Effects, GossipTarget,
    },
    reactor::{Finalize, ReactorEvent},
    tls,
    types::{NodeId, ValidatorMatrix},
    utils::{
        self, display_error, DropSwitch, Fuse, LockedLineWriter, ObservableFuse, Source,
        TokenizedCount,
    },
    NodeRng,
};

use super::ValidatorBoundComponent;

const COMPONENT_NAME: &str = "network";

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

/// How often to send a ping down a healthy connection.
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum time for a ping until it connections are severed.
///
/// If you are running a network under very extreme conditions, it may make sense to alter these
/// values, but usually these values should require no changing.
///
/// `PING_TIMEOUT` should be less than `PING_INTERVAL` at all times.
const PING_TIMEOUT: Duration = Duration::from_secs(6);

/// How many pings to send before giving up and dropping the connection.
const PING_RETRIES: u16 = 5;

#[derive(Clone, DataSize, Debug)]
pub(crate) struct OutgoingHandle {
    #[data_size(skip)] // Unfortunately, there is no way to inspect an `UnboundedSender`.
    rpc_client: JulietRpcClient<{ Channel::COUNT }>,
    peer_addr: SocketAddr,
}

impl Display for OutgoingHandle {
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
    /// A reference to the global validator matrix.
    validator_matrix: ValidatorMatrix,

    /// Outgoing connections manager.
    outgoing_manager: OutgoingManager<OutgoingHandle, ConnectionError>,
    /// Incoming validator map.
    ///
    /// Tracks which incoming connections are from validators. The atomic bool is shared with the
    /// receiver tasks to determine queue position.
    incoming_validator_status: HashMap<PublicKey, Weak<AtomicBool>>,
    /// Tracks whether a connection is symmetric or not.
    connection_symmetries: HashMap<NodeId, ConnectionSymmetry>,

    /// Fuse signaling a shutdown of the small network.
    shutdown_fuse: DropSwitch<ObservableFuse>,

    /// Join handle for the server thread.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,

    /// Builder for new node-to-node RPC instances.
    #[data_size(skip)]
    rpc_builder: RpcBuilder<{ Channel::COUNT }>,

    /// Networking metrics.
    #[data_size(skip)]
    net_metrics: Arc<Metrics>,

    /// The era that is considered the active era by the network component.
    active_era: EraId,

    /// The state of this component.
    state: ComponentState,

    /// Marker for what kind of payload this small network instance supports.
    _payload: PhantomData<P>,
}

impl<REv, P> Network<REv, P>
where
    P: Payload,
    REv: ReactorEvent
        + From<Event<P>>
        + FromIncoming<P>
        + From<StorageRequest>
        + From<NetworkRequest<P>>
        + From<PeerBehaviorAnnouncement>
        + From<BeginGossipRequest<GossipedAddress>>,
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
    ) -> Result<Network<REv, P>, Error> {
        let net_metrics = Arc::new(Metrics::new(registry)?);

        let outgoing_manager = OutgoingManager::with_metrics(
            OutgoingConfig {
                retry_attempts: RECONNECTION_ATTEMPTS,
                base_timeout: BASE_RECONNECTION_TIMEOUT,
                unblock_after: cfg.blocklist_retain_duration.into(),
                sweep_timeout: cfg.max_addr_pending_time.into(),
                health: HealthConfig {
                    ping_interval: PING_INTERVAL,
                    ping_timeout: PING_TIMEOUT,
                    ping_retries: PING_RETRIES,
                    pong_limit: (1 + PING_RETRIES as u32) * 2,
                },
            },
            net_metrics.create_outgoing_metrics(),
        );

        let keylog = match cfg.keylog_path {
            Some(ref path) => {
                let keylog = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .write(true)
                    .open(path)
                    .map_err(Error::CannotAppendToKeylog)?;
                warn!(%path, "keylog enabled, if you are not debugging turn this off in your configuration (`network.keylog_path`)");
                Some(LockedLineWriter::new(keylog))
            }
            None => None,
        };

        let chain_info = chain_info_source.into();
        let rpc_builder = transport::create_rpc_builder(
            chain_info.maximum_net_message_size,
            cfg.max_in_flight_demands,
        );

        let context = Arc::new(NetworkContext::new(
            cfg.clone(),
            our_identity,
            keylog,
            node_key_pair.map(NodeKeyPair::new),
            chain_info,
            &net_metrics,
        ));

        let component = Network {
            cfg,
            context,
            validator_matrix,
            outgoing_manager,
            incoming_validator_status: Default::default(),
            connection_symmetries: HashMap::new(),
            net_metrics,
            // We start with an empty set of validators for era 0 and expect to be updated.
            active_era: EraId::new(0),
            state: ComponentState::Uninitialized,
            shutdown_fuse: DropSwitch::new(ObservableFuse::new()),
            server_join_handle: None,
            rpc_builder,
            _payload: PhantomData,
        };

        Ok(component)
    }

    fn initialize(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Event<P>>, Error> {
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

        let context = self.context.clone();
        self.server_join_handle = Some(tokio::spawn(
            tasks::server(
                context,
                tokio::net::TcpListener::from_std(listener).map_err(Error::ListenerConversion)?,
                self.shutdown_fuse.inner().clone(),
            )
            .in_current_span(),
        ));

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

        <Self as InitializedComponent<REv>>::set_state(self, ComponentState::Initialized);
        Ok(effects)
    }

    /// Queues a message to be sent to validator nodes in the given era.
    fn broadcast_message_to_validators(&self, msg: Arc<Message<P>>, era_id: EraId) {
        self.net_metrics.broadcast_requests.inc();

        let mut total_connected_validators_in_era = 0;
        let mut total_outgoing_manager_connected_peers = 0;

        for peer_id in self.outgoing_manager.connected_peers() {
            total_outgoing_manager_connected_peers += 1;

            if true {
                total_connected_validators_in_era += 1;
                self.send_message(peer_id, msg.clone(), None)
            }
        }

        debug!(
            msg = %msg,
            era = era_id.value(),
            total_connected_validators_in_era,
            total_outgoing_manager_connected_peers,
            "broadcast_message_to_validators"
        );
    }

    /// Queues a message to `count` random nodes on the network.
    fn gossip_message(
        &self,
        rng: &mut NodeRng,
        msg: Arc<Message<P>>,
        _gossip_target: GossipTarget,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        // TODO: Restore sampling functionality. We currently override with `GossipTarget::All`.
        //       See #4247.
        let is_validator_in_era = |_, _: &_| true;
        let gossip_target = GossipTarget::All;

        let peer_ids = choose_gossip_peers(
            rng,
            gossip_target,
            count,
            exclude.clone(),
            self.outgoing_manager.connected_peers(),
            is_validator_in_era,
        );

        // todo!() - consider sampling more validators (for example: 10%, but not fewer than 5)

        if peer_ids.len() != count {
            let not_excluded = self
                .outgoing_manager
                .connected_peers()
                .filter(|peer_id| !exclude.contains(peer_id))
                .count();
            if not_excluded > 0 {
                let connected = self.outgoing_manager.connected_peers().count();
                debug!(
                    our_id=%self.context.our_id(),
                    %gossip_target,
                    wanted = count,
                    connected,
                    not_excluded,
                    selected = peer_ids.len(),
                    "could not select enough random nodes for gossiping"
                );
            }
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
        message_queued_responder: Option<AutoClosingResponder<()>>,
    ) {
        // Try to send the message.
        if let Some(connection) = self.outgoing_manager.get_route(dest) {
            let channel = msg.get_channel();

            let payload = if let Some(payload) = serialize_network_message(&msg) {
                payload
            } else {
                // No need to log, `serialize_network_message` already logs the failure.
                return;
            };
            trace!(%msg, encoded_size=payload.len(), %channel, "enqueing message for sending");

            let channel_id = channel.into_channel_id();
            let request = connection
                .rpc_client
                .create_request(channel_id)
                .with_payload(payload);

            if let Some(responder) = message_queued_responder {
                // Technically, the queueing future should be spawned by the reactor, but we can
                //       make a case here since the networking component usually controls its own
                //       futures, we are allowed to spawn these as well.
                tokio::spawn(async move {
                    let guard = request.queue_for_sending().await;
                    responder.respond(()).await;

                    // We need to properly process the guard, so it does not cause a cancellation.
                    process_request_guard(channel, guard)
                });
            } else {
                // No responder given, so we do a best effort of sending the message.
                match request.try_queue_for_sending() {
                    Ok(guard) => process_request_guard(channel, guard),
                    Err(builder) => {
                        // We had to drop the message, since we hit the buffer limit.
                        debug!(%channel, "node is sending at too high a rate, message dropped");

                        let payload = builder.into_payload().unwrap_or_default();
                        match deserialize_network_message::<P>(&payload) {
                            Ok(reconstructed_message) => {
                                debug!(our_id=%self.context.our_id(), %dest, msg=%reconstructed_message, "dropped outgoing message, buffer exhausted");
                            }
                            Err(err) => {
                                error!(our_id=%self.context.our_id(),
                                       %dest,
                                       reconstruction_error=%err,
                                       ?payload,
                                       "dropped outgoing message, buffer exhausted and also failed to reconstruct it"
                                );
                            }
                        }
                    }
                }
            }

            let _send_token = TokenizedCount::new(self.net_metrics.queued_messages.inner().clone());
            // TODO: How to update self.net_metrics.queued_messages? Or simply remove metric?
        } else {
            // We are not connected, so the reconnection is likely already in progress.
            debug!(our_id=%self.context.our_id(), %dest, ?msg, "dropped outgoing message, no connection");
        }
    }

    fn handle_incoming_connection(
        &mut self,
        incoming: Box<IncomingConnection>,
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
                transport,
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

                // If given a key, determine validator status.
                let validator_status = peer_consensus_public_key
                    .as_ref()
                    .map(|public_key| {
                        let status = self
                            .validator_matrix
                            .is_active_or_upcoming_validator(public_key);

                        // Find the shared `Arc` that holds validator status for this specific key.
                        match self.incoming_validator_status.entry((**public_key).clone()) {
                            // TODO: Use `Arc` for public key-key.
                            Entry::Occupied(mut occupied) => {
                                match occupied.get().upgrade() {
                                    Some(arc) => {
                                        arc.store(status, Ordering::Relaxed);
                                        arc
                                    }
                                    None => {
                                        // Failed to ugprade, the weak pointer is just a leftover
                                        // that has not been cleaned up yet. We can replace it.
                                        let arc = Arc::new(AtomicBool::new(status));
                                        occupied.insert(Arc::downgrade(&arc));
                                        arc
                                    }
                                }
                            }
                            Entry::Vacant(vacant) => {
                                let arc = Arc::new(AtomicBool::new(status));
                                vacant.insert(Arc::downgrade(&arc));
                                arc
                            }
                        }
                    })
                    .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

                let (read_half, write_half) = tokio::io::split(transport);

                let (rpc_client, rpc_server) = self.rpc_builder.build(read_half, write_half);

                // Now we can start the message reader.
                let boxed_span = Box::new(span.clone());
                effects.extend(
                    tasks::message_receiver(
                        self.context.clone(),
                        validator_status,
                        rpc_server,
                        self.shutdown_fuse.inner().clone(),
                        peer_id,
                        span.clone(),
                    )
                    .instrument(span)
                    .event(move |result| {
                        // By moving the `rpc_client` into this closure to drop it, we ensure it
                        // does not get dropped until after `tasks::message_receiver` has returned.
                        // This is important because dropping `rpc_client` is one of the ways to
                        // trigger a connection shutdown from our end.
                        drop(rpc_client);

                        Event::IncomingClosed {
                            result: result.map_err(Box::new),
                            peer_id: Box::new(peer_id),
                            peer_addr,
                            peer_consensus_public_key,
                            span: boxed_span,
                        }
                    }),
                );

                effects
            }
        })
    }

    fn handle_incoming_closed(
        &mut self,
        result: Result<(), Box<MessageReceiverError>>,
        peer_id: Box<NodeId>,
        peer_addr: SocketAddr,
        peer_consensus_public_key: Option<Box<PublicKey>>,
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

            // Update the connection symmetries and cleanup if necessary.
            if !self
                .connection_symmetries
                .entry(*peer_id)
                .or_default() // Should never occur.
                .remove_incoming(peer_addr, Instant::now())
            {
                if let Some(ref public_key) = peer_consensus_public_key {
                    self.incoming_validator_status.remove(public_key);
                }

                self.connection_symmetries.remove(&peer_id);
            }

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
            | ConnectionError::IncompatibleVersion(_)
            | ConnectionError::HandshakeTimeout => None,

            // These errors are potential bugs on our side.
            ConnectionError::HandshakeSenderCrashed(_)
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
        outgoing: OutgoingConnection,
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
                peer_consensus_public_key: _, // TODO: Use for limiting or remove. See also #4247.
                transport,
            } => {
                info!("new outgoing connection established");

                let (read_half, write_half) = tokio::io::split(transport);

                let (rpc_client, rpc_server) = self.rpc_builder.build(read_half, write_half);

                let handle = OutgoingHandle {
                    rpc_client,
                    peer_addr,
                };

                let request = self
                    .outgoing_manager
                    .handle_dial_outcome(DialOutcome::Successful {
                        addr: peer_addr,
                        handle,
                        node_id: peer_id,
                        when: now,
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
                }

                effects.extend(tasks::rpc_sender_loop(rpc_server).instrument(span).event(
                    move |_| Event::OutgoingDropped {
                        peer_id: Box::new(peer_id),
                        peer_addr,
                    },
                ));

                effects
            }
        })
    }

    fn handle_network_request(
        &self,
        request: NetworkRequest<P>,
        rng: &mut NodeRng,
    ) -> Effects<Event<P>> {
        match request {
            NetworkRequest::SendMessage {
                dest,
                payload,
                message_queued_responder,
            } => {
                // We're given a message to send. Pass on the responder so that confirmation
                // can later be given once the message has actually been buffered.
                self.net_metrics.direct_message_requests.inc();

                self.send_message(
                    *dest,
                    Arc::new(Message::Payload(*payload)),
                    message_queued_responder,
                );
                Effects::new()
            }
            NetworkRequest::ValidatorBroadcast {
                payload,
                era_id,
                auto_closing_responder,
            } => {
                // We're given a message to broadcast.
                self.broadcast_message_to_validators(Arc::new(Message::Payload(*payload)), era_id);
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

        self.process_dial_requests(requests)
    }

    /// Processes a set of `DialRequest`s, updating the component and emitting needed effects.
    fn process_dial_requests<T>(&mut self, requests: T) -> Effects<Event<P>>
    where
        T: IntoIterator<Item = DialRequest<OutgoingHandle>>,
    {
        let mut effects = Effects::new();

        for request in requests.into_iter() {
            trace!(%request, "processing dial request");
            match request {
                DialRequest::Dial { addr, span } => effects.extend(
                    tasks::connect_outgoing::<P, _>(self.context.clone(), addr)
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
                DialRequest::SendPing {
                    peer_id,
                    nonce,
                    span,
                } => span.in_scope(|| {
                    trace!("enqueuing ping to be sent");
                    self.send_message(peer_id, Arc::new(Message::Ping { nonce }), None);
                }),
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
        ticket: Ticket,
        span: Span,
    ) -> Effects<Event<P>>
    where
        REv: FromIncoming<P> + From<NetworkRequest<P>> + From<PeerBehaviorAnnouncement>,
    {
        // Note: For non-payload channels, we drop the `Ticket` implicitly at end of scope.
        span.in_scope(|| match msg {
            Message::Handshake { .. } => {
                // We should never receive a handshake message on an established connection. Simply
                // discard it. This may be too lenient, so we may consider simply dropping the
                // connection in the future instead.
                warn!("received unexpected handshake");
                Effects::new()
            }
            Message::Ping { nonce } => {
                // Send a pong. Incoming pings and pongs are rate limited.

                self.send_message(peer_id, Arc::new(Message::Pong { nonce }), None);
                Effects::new()
            }
            Message::Pong { nonce } => {
                // Record the time the pong arrived and forward it to outgoing.
                let pong = TaggedTimestamp::from_parts(Instant::now(), nonce);
                if self.outgoing_manager.record_pong(peer_id, pong) {
                    effect_builder
                        .announce_block_peer_with_justification(
                            peer_id,
                            BlocklistJustification::PongLimitExceeded,
                        )
                        .ignore()
                } else {
                    Effects::new()
                }
            }
            Message::Payload(payload) => effect_builder
                .announce_incoming(peer_id, payload, ticket)
                .ignore(),
        })
    }

    /// Emits an announcement that a connection has been completed.
    fn connection_completed(&self, peer_id: NodeId) {
        trace!(num_peers = self.peers().len(), new_peer=%peer_id, "connection complete");
        self.net_metrics.peers.set(self.peers().len() as i64);
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
            self.shutdown_fuse.inner().set();

            // Wait for the server to exit cleanly.
            if let Some(join_handle) = self.server_join_handle.take() {
                match join_handle.await {
                    Ok(_) => debug!(our_id=%self.context.our_id(), "server exited cleanly"),
                    Err(ref err) => {
                        error!(our_id=%self.context.our_id(), err=display_error(err), "could not join server task cleanly")
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

fn choose_gossip_peers<F>(
    rng: &mut NodeRng,
    gossip_target: GossipTarget,
    count: usize,
    exclude: HashSet<NodeId>,
    connected_peers: impl Iterator<Item = NodeId>,
    is_validator_in_era: F,
) -> HashSet<NodeId>
where
    F: Fn(EraId, &NodeId) -> bool,
{
    let filtered_peers = connected_peers.filter(|peer_id| !exclude.contains(peer_id));
    match gossip_target {
        GossipTarget::Mixed(era_id) => {
            let (validators, non_validators): (Vec<_>, Vec<_>) =
                filtered_peers.partition(|node_id| is_validator_in_era(era_id, node_id));

            let (first, second) = if rng.gen() {
                (validators, non_validators)
            } else {
                (non_validators, validators)
            };

            first
                .choose_multiple(rng, count)
                .interleave(second.iter().choose_multiple(rng, count))
                .take(count)
                .copied()
                .collect()
        }
        GossipTarget::All => filtered_peers
            .choose_multiple(rng, count)
            .into_iter()
            .collect(),
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
        match &self.state {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as Component<REv>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<REv>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => match self.initialize(effect_builder) {
                    Ok(effects) => effects,
                    Err(error) => {
                        error!(%error, "failed to initialize network component");
                        <Self as InitializedComponent<REv>>::set_state(
                            self,
                            ComponentState::Fatal(error.to_string()),
                        );
                        Effects::new()
                    }
                },
                Event::IncomingConnection { .. }
                | Event::IncomingMessage { .. }
                | Event::IncomingClosed { .. }
                | Event::OutgoingConnection { .. }
                | Event::OutgoingDropped { .. }
                | Event::NetworkRequest { .. }
                | Event::NetworkInfoRequest { .. }
                | Event::GossipOurAddress
                | Event::PeerAddressReceived(_)
                | Event::SweepOutgoing
                | Event::BlocklistAnnouncement(_) => {
                    warn!(
                        ?event,
                        name = <Self as Component<REv>>::name(self),
                        "should not handle this event when component is pending initialization"
                    );
                    Effects::new()
                }
            },
            ComponentState::Initialized => match event {
                Event::Initialize => {
                    error!(
                        ?event,
                        name = <Self as Component<REv>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::IncomingConnection { incoming, span } => {
                    self.handle_incoming_connection(incoming, span)
                }
                Event::IncomingMessage {
                    peer_id,
                    msg,
                    span,
                    ticket,
                } => self.handle_incoming_message(effect_builder, *peer_id, *msg, ticket, span),
                Event::IncomingClosed {
                    result,
                    peer_id,
                    peer_addr,
                    peer_consensus_public_key,
                    span,
                } => self.handle_incoming_closed(
                    result,
                    peer_id,
                    peer_addr,
                    peer_consensus_public_key,
                    *span,
                ),
                Event::OutgoingConnection { outgoing, span } => {
                    self.handle_outgoing_connection(*outgoing, span)
                }
                Event::OutgoingDropped { peer_id, peer_addr } => {
                    self.handle_outgoing_dropped(*peer_id, peer_addr)
                }
                Event::NetworkRequest { req: request } => {
                    self.handle_network_request(*request, rng)
                }
                Event::NetworkInfoRequest { req } => {
                    return match *req {
                        NetworkInfoRequest::Peers { responder } => {
                            responder.respond(self.peers()).ignore()
                        }
                        NetworkInfoRequest::FullyConnectedPeers { count, responder } => responder
                            .respond(self.fully_connected_peers_random(rng, count))
                            .ignore(),
                        NetworkInfoRequest::Insight { responder } => responder
                            .respond(NetworkInsights::collect_from_component(self))
                            .ignore(),
                    }
                }
                Event::GossipOurAddress => {
                    let our_address = GossipedAddress::new(
                        self.context
                            .public_addr()
                            .expect("component not initialized properly"),
                    );

                    let mut effects = effect_builder
                        .begin_gossip(our_address, Source::Ourself, our_address.gossip_target())
                        .ignore();
                    effects.extend(
                        effect_builder
                            .set_timeout(self.cfg.gossip_interval.into())
                            .event(|_| Event::GossipOurAddress),
                    );
                    effects
                }
                Event::PeerAddressReceived(gossiped_address) => {
                    let requests = self.outgoing_manager.learn_addr(
                        gossiped_address.into(),
                        false,
                        Instant::now(),
                    );
                    self.process_dial_requests(requests)
                }
                Event::SweepOutgoing => {
                    let now = Instant::now();
                    let requests = self.outgoing_manager.perform_housekeeping(rng, now);

                    let mut effects = self.process_dial_requests(requests);

                    effects.extend(
                        effect_builder
                            .set_timeout(OUTGOING_MANAGER_SWEEP_INTERVAL)
                            .event(|_| Event::SweepOutgoing),
                    );

                    effects
                }
                Event::BlocklistAnnouncement(announcement) => match announcement {
                    PeerBehaviorAnnouncement::OffenseCommitted {
                        offender,
                        justification,
                    } => {
                        // TODO: We do not have a proper by-node-ID blocklist, but rather only block
                        // the current outgoing address of a peer.
                        info!(%offender, %justification, "adding peer to blocklist after transgression");

                        if let Some(addr) = self.outgoing_manager.get_addr(*offender) {
                            let requests = self.outgoing_manager.block_addr(
                                addr,
                                Instant::now(),
                                *justification,
                            );
                            self.process_dial_requests(requests)
                        } else {
                            // Peer got away with it, no longer an outgoing connection.
                            Effects::new()
                        }
                    }
                },
            },
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
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
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<REv>>::name(self),
            "component state changed"
        );

        self.state = new_state;
    }
}

impl<REv, P> ValidatorBoundComponent<REv> for Network<REv, P>
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
    fn handle_validators(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
    ) -> Effects<Self::Event> {
        // If we receive an updated set of validators, recalculate validator status for every
        // existing connection.

        let active_validators = self.validator_matrix.active_or_upcoming_validators();

        // Update the validator status for every connection.
        for (public_key, status) in self.incoming_validator_status.iter_mut() {
            // If there is only a `Weak` ref, we lost the connection to the validator, but the
            // disconnection has not reached us yet.
            if let Some(arc) = status.upgrade() {
                arc.store(
                    active_validators.contains(public_key),
                    std::sync::atomic::Ordering::Relaxed,
                )
            }
        }

        Effects::default()
    }
}

/// Transport type for base encrypted connections.
type Transport = SslStream<TcpStream>;

/// Transport-level RPC server.
type RpcServer = JulietRpcServer<
    { Channel::COUNT },
    ReadHalf<SslStream<TcpStream>>,
    WriteHalf<SslStream<TcpStream>>,
>;

/// Setups bincode encoding used on the networking transport.
fn bincode_config() -> impl Options {
    bincode::options()
        .with_no_limit() // We rely on `juliet` to impose limits.
        .with_little_endian() // Default at the time of this writing, we are merely pinning it.
        .with_varint_encoding() // Same as above.
        .reject_trailing_bytes() // There is no reason for us not to reject trailing bytes.
}

/// Serializes a network message with the protocol specified encoding.
///
/// This function exists as a convenience, because there never should be a failure in serializing
/// messages we produced ourselves.
fn serialize_network_message<T>(msg: &T) -> Option<Bytes>
where
    T: Serialize + ?Sized,
{
    bincode_config()
        .serialize(msg)
        .map(Bytes::from)
        .map_err(|err| {
            error!(%err, "serialization failure when encoding outgoing message");
            err
        })
        .ok()
}

/// Deserializes a networking message from the protocol specified encoding.
fn deserialize_network_message<P>(bytes: &[u8]) -> Result<Message<P>, bincode::Error>
where
    P: Payload,
{
    bincode_config().deserialize(bytes)
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
            .field("state", &self.state)
            .field("public_addr", &self.context.public_addr())
            .finish()
    }
}

/// Processes a request guard obtained by making a request to a peer through Juliet RPC.
///
/// Ensures that outgoing messages are not cancelled, a would be the case when simply dropping the
/// `RequestGuard`. Potential errors that are available early are dropped, later errors discarded.
#[inline]
fn process_request_guard(channel: Channel, guard: RequestGuard) {
    match guard.try_wait_for_response() {
        Ok(Ok(_outcome)) => {
            // We got an incredibly quick round-trip, lucky us! Nothing to do.
        }
        Ok(Err(err)) => {
            debug!(%channel, %err, "failed to send message");
        }
        Err(guard) => {
            // No ACK received yet, forget, so we don't cancel.
            guard.forget();
        }
    }
}

#[cfg(test)]
mod gossip_target_tests {
    use std::{collections::BTreeSet, iter};

    use static_assertions::const_assert;

    use casper_types::testing::TestRng;

    use super::*;

    const VALIDATOR_COUNT: usize = 10;
    const NON_VALIDATOR_COUNT: usize = 20;
    // The tests assume that we have fewer validators than non-validators.
    const_assert!(VALIDATOR_COUNT < NON_VALIDATOR_COUNT);

    struct Fixture {
        validators: BTreeSet<NodeId>,
        non_validators: BTreeSet<NodeId>,
        all_peers: Vec<NodeId>,
    }

    impl Fixture {
        fn new(rng: &mut TestRng) -> Self {
            let validators: BTreeSet<NodeId> = iter::repeat_with(|| NodeId::random(rng))
                .take(VALIDATOR_COUNT)
                .collect();
            let non_validators: BTreeSet<NodeId> = iter::repeat_with(|| NodeId::random(rng))
                .take(NON_VALIDATOR_COUNT)
                .collect();

            let mut all_peers: Vec<NodeId> = validators
                .iter()
                .copied()
                .chain(non_validators.iter().copied())
                .collect();
            all_peers.shuffle(rng);

            Fixture {
                validators,
                non_validators,
                all_peers,
            }
        }

        fn is_validator_in_era(&self) -> impl Fn(EraId, &NodeId) -> bool + '_ {
            move |_era_id: EraId, node_id: &NodeId| self.validators.contains(node_id)
        }

        fn num_validators<'a>(&self, input: impl Iterator<Item = &'a NodeId>) -> usize {
            input
                .filter(move |&node_id| self.validators.contains(node_id))
                .count()
        }

        fn num_non_validators<'a>(&self, input: impl Iterator<Item = &'a NodeId>) -> usize {
            input
                .filter(move |&node_id| self.non_validators.contains(node_id))
                .count()
        }
    }

    #[test]
    fn should_choose_mixed() {
        const TARGET: GossipTarget = GossipTarget::Mixed(EraId::new(1));

        let mut rng = TestRng::new();
        let fixture = Fixture::new(&mut rng);

        // Choose more than total count from all peers, exclude none, should return all peers.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT + NON_VALIDATOR_COUNT + 1,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), fixture.all_peers.len());

        // Choose total count from all peers, exclude none, should return all peers.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT + NON_VALIDATOR_COUNT,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), fixture.all_peers.len());

        // Choose 2 * VALIDATOR_COUNT from all peers, exclude none, should return all validators and
        // VALIDATOR_COUNT non-validators.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            2 * VALIDATOR_COUNT,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), 2 * VALIDATOR_COUNT);
        assert_eq!(fixture.num_validators(chosen.iter()), VALIDATOR_COUNT);
        assert_eq!(fixture.num_non_validators(chosen.iter()), VALIDATOR_COUNT);

        // Choose VALIDATOR_COUNT from all peers, exclude none, should return VALIDATOR_COUNT peers,
        // half validators and half non-validators.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), VALIDATOR_COUNT);
        assert_eq!(fixture.num_validators(chosen.iter()), VALIDATOR_COUNT / 2);
        assert_eq!(
            fixture.num_non_validators(chosen.iter()),
            VALIDATOR_COUNT / 2
        );

        // Choose two from all peers, exclude none, should return two peers, one validator and one
        // non-validator.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            2,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), 2);
        assert_eq!(fixture.num_validators(chosen.iter()), 1);
        assert_eq!(fixture.num_non_validators(chosen.iter()), 1);

        // Choose one from all peers, exclude none, should return one peer with 50-50 chance of
        // being a validator.
        let mut got_validator = false;
        let mut got_non_validator = false;
        let mut attempts = 0;
        while !got_validator || !got_non_validator {
            let chosen = choose_gossip_peers(
                &mut rng,
                TARGET,
                1,
                HashSet::new(),
                fixture.all_peers.iter().copied(),
                fixture.is_validator_in_era(),
            );
            assert_eq!(chosen.len(), 1);
            let node_id = chosen.iter().next().unwrap();
            got_validator |= fixture.validators.contains(node_id);
            got_non_validator |= fixture.non_validators.contains(node_id);
            attempts += 1;
            assert!(attempts < 1_000_000);
        }

        // Choose VALIDATOR_COUNT from all peers, exclude all but one validator, should return the
        // one validator and VALIDATOR_COUNT - 1 non-validators.
        let exclude: HashSet<_> = fixture
            .validators
            .iter()
            .copied()
            .take(VALIDATOR_COUNT - 1)
            .collect();
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT,
            exclude.clone(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), VALIDATOR_COUNT);
        assert_eq!(fixture.num_validators(chosen.iter()), 1);
        assert_eq!(
            fixture.num_non_validators(chosen.iter()),
            VALIDATOR_COUNT - 1
        );
        assert!(exclude.is_disjoint(&chosen));

        // Choose 3 from all peers, exclude all non-validators, should return 3 validators.
        let exclude: HashSet<_> = fixture.non_validators.iter().copied().collect();
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            3,
            exclude.clone(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), 3);
        assert_eq!(fixture.num_validators(chosen.iter()), 3);
        assert!(exclude.is_disjoint(&chosen));
    }

    #[test]
    fn should_choose_all() {
        const TARGET: GossipTarget = GossipTarget::All;

        let mut rng = TestRng::new();
        let fixture = Fixture::new(&mut rng);

        // Choose more than total count from all peers, exclude none, should return all peers.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT + NON_VALIDATOR_COUNT + 1,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), fixture.all_peers.len());

        // Choose total count from all peers, exclude none, should return all peers.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT + NON_VALIDATOR_COUNT,
            HashSet::new(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), fixture.all_peers.len());

        // Choose VALIDATOR_COUNT from only validators, exclude none, should return all validators.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT,
            HashSet::new(),
            fixture.validators.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), VALIDATOR_COUNT);
        assert_eq!(fixture.num_validators(chosen.iter()), VALIDATOR_COUNT);

        // Choose VALIDATOR_COUNT from only non-validators, exclude none, should return
        // VALIDATOR_COUNT non-validators.
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT,
            HashSet::new(),
            fixture.non_validators.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), VALIDATOR_COUNT);
        assert_eq!(fixture.num_non_validators(chosen.iter()), VALIDATOR_COUNT);

        // Choose VALIDATOR_COUNT from all peers, exclude all but VALIDATOR_COUNT from all peers,
        // should return all the non-excluded peers.
        let exclude: HashSet<_> = fixture
            .all_peers
            .iter()
            .copied()
            .take(NON_VALIDATOR_COUNT)
            .collect();
        let chosen = choose_gossip_peers(
            &mut rng,
            TARGET,
            VALIDATOR_COUNT,
            exclude.clone(),
            fixture.all_peers.iter().copied(),
            fixture.is_validator_in_era(),
        );
        assert_eq!(chosen.len(), VALIDATOR_COUNT);
        assert!(exclude.is_disjoint(&chosen));

        // Choose one from all peers, exclude enough non-validators to have an even chance of
        // returning a validator as a non-validator, should return one peer with 50-50 chance of
        // being a validator.
        let exclude: HashSet<_> = fixture
            .non_validators
            .iter()
            .copied()
            .take(NON_VALIDATOR_COUNT - VALIDATOR_COUNT)
            .collect();
        let mut got_validator = false;
        let mut got_non_validator = false;
        let mut attempts = 0;
        while !got_validator || !got_non_validator {
            let chosen = choose_gossip_peers(
                &mut rng,
                TARGET,
                1,
                exclude.clone(),
                fixture.all_peers.iter().copied(),
                fixture.is_validator_in_era(),
            );
            assert_eq!(chosen.len(), 1);
            assert!(exclude.is_disjoint(&chosen));
            let node_id = chosen.iter().next().unwrap();
            got_validator |= fixture.validators.contains(node_id);
            got_non_validator |= fixture.non_validators.contains(node_id);
            attempts += 1;
            assert!(attempts < 1_000_000);
        }
    }
}
