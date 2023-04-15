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
mod limiter;
mod message;
mod metrics;
mod outgoing;
mod symmetry;
pub(crate) mod tasks;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    fs::OpenOptions,
    io,
    marker::PhantomData,
    net::{SocketAddr, TcpListener},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use array_init::array_init;
use bincode::Options;
use bytes::Bytes;
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use itertools::Itertools;
use muxink::{
    backpressured::{BackpressuredSink, BackpressuredSinkError, BackpressuredStream, Ticket},
    demux::{Demultiplexer, DemultiplexerError, DemultiplexerHandle},
    fragmented::{Defragmentizer, Fragmentizer, SingleFragment},
    framing::{fixed_size::FixedSize, length_delimited::LengthDelimited},
    io::{FrameReader, FrameWriter},
    little_endian::{DecodeError, LittleEndian},
    mux::{ChannelPrefixedFrame, Multiplexer, MultiplexerError, MultiplexerHandle},
    ImmediateFrameU64,
};

use prometheus::Registry;
use rand::{
    seq::{IteratorRandom, SliceRandom},
    Rng,
};
use strum::EnumCount;
use tokio::{
    io::{ReadHalf, WriteHalf},
    net::TcpStream,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_openssl::SslStream;
use tokio_util::compat::Compat;
use tracing::{debug, error, info, trace, warn, Instrument, Span};

use casper_types::{EraId, PublicKey, SecretKey};

use self::{
    blocklist::BlocklistJustification,
    chain_info::ChainInfo,
    error::{ConnectionError, MessageReaderError},
    event::{IncomingConnection, OutgoingConnection},
    health::{HealthConfig, TaggedTimestamp},
    limiter::Limiter,
    message::NodeKeyPair,
    metrics::Metrics,
    outgoing::{DialOutcome, DialRequest, OutgoingConfig, OutgoingManager},
    symmetry::ConnectionSymmetry,
    tasks::{EncodedMessage, NetworkContext},
};
pub(crate) use self::{
    config::Config,
    error::Error,
    event::Event,
    gossiped_address::GossipedAddress,
    identity::Identity,
    insights::NetworkInsights,
    message::{Channel, EstimatorWeights, FromIncoming, Message, MessageKind, Payload},
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

/// The size of a single message fragment sent over the wire.
const MESSAGE_FRAGMENT_SIZE: usize = 4096;

/// How many bytes of ACKs to read in one go.
const ACK_BUFFER_SIZE: usize = 1024;

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

/// How many items to buffer before backpressuring.
// TODO: This should probably be configurable on a per-channel basis.
const BACKPRESSURE_WINDOW_SIZE: u64 = 20;

#[derive(Clone, DataSize, Debug)]
pub(crate) struct OutgoingHandle {
    #[data_size(skip)] // Unfortunately, there is no way to inspect an `UnboundedSender`.
    senders: [UnboundedSender<EncodedMessage>; Channel::COUNT],
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

    /// Outgoing connections manager.
    outgoing_manager: OutgoingManager<OutgoingHandle, ConnectionError>,
    /// Tracks whether a connection is symmetric or not.
    connection_symmetries: HashMap<NodeId, ConnectionSymmetry>,

    /// Fuse signaling a shutdown of the small network.
    shutdown_fuse: DropSwitch<ObservableFuse>,

    /// Join handle for the server thread.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,

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

        let outgoing_limiter = Limiter::new(
            cfg.max_outgoing_byte_rate_non_validators,
            net_metrics
                .accumulated_outgoing_limiter_delay
                .inner()
                .clone(),
            validator_matrix.clone(),
        );

        let incoming_limiter = Limiter::new(
            cfg.max_incoming_message_rate_non_validators,
            net_metrics
                .accumulated_incoming_limiter_delay
                .inner()
                .clone(),
            validator_matrix,
        );

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

        let context = Arc::new(NetworkContext::new(
            cfg.clone(),
            our_identity,
            keylog,
            node_key_pair.map(NodeKeyPair::new),
            chain_info_source.into(),
            &net_metrics,
        ));

        let component = Network {
            cfg,
            context,
            outgoing_manager,
            connection_symmetries: HashMap::new(),
            net_metrics,
            outgoing_limiter,
            incoming_limiter,
            // We start with an empty set of validators for era 0 and expect to be updated.
            active_era: EraId::new(0),
            state: ComponentState::Uninitialized,
            shutdown_fuse: DropSwitch::new(ObservableFuse::new()),
            server_join_handle: None,
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
            if self.outgoing_limiter.is_validator_in_era(era_id, &peer_id) {
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
        gossip_target: GossipTarget,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let is_validator_in_era =
            |era: EraId, peer_id: &NodeId| self.outgoing_limiter.is_validator_in_era(era, peer_id);
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
        opt_responder: Option<AutoClosingResponder<()>>,
    ) {
        // Try to send the message.
        if let Some(connection) = self.outgoing_manager.get_route(dest) {
            let channel = msg.get_channel();
            let sender = &connection.senders[channel as usize];
            let payload = if let Some(payload) = serialize_network_message(&msg) {
                payload
            } else {
                // The `AutoClosingResponder` will respond by itself.
                return;
            };
            trace!(%msg, encoded_size=payload.len(), %channel, "enqueing message for sending");

            let send_token = TokenizedCount::new(self.net_metrics.queued_messages.inner().clone());

            if let Err(refused_message) =
                sender.send(EncodedMessage::new(payload, opt_responder, send_token))
            {
                match deserialize_network_message::<P>(refused_message.0.payload()) {
                    Ok(reconstructed_message) => {
                        // We lost the connection, but that fact has not reached us as an event yet.
                        debug!(our_id=%self.context.our_id(), %dest, msg=%reconstructed_message, "dropped outgoing message, lost connection");
                    }
                    Err(err) => {
                        error!(our_id=%self.context.our_id(),
                               %dest,
                               reconstruction_error=%err,
                               payload=?refused_message.0.payload(),
                               "dropped outgoing message, but also failed to reconstruct it"
                        );
                    }
                }
            }
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

                // TODO: Removal of `CountingTransport` here means some functionality has to be
                // restored.

                let (read_half, write_half) = tokio::io::split(transport);

                // Setup a multiplexed delivery for ACKs (we use the send direction of the incoming
                // connection for sending ACKs only).
                let write_compat: Compat<WriteHalf<SslStream<TcpStream>>> =
                    tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(write_half);

                let ack_writer: AckFrameWriter =
                    FrameWriter::new(FixedSize::new(ACK_FRAME_SIZE), write_compat);
                let ack_carrier = Multiplexer::new(ack_writer);

                // `rust-openssl` does not support the futures 0.3 `AsyncRead` trait (it uses the
                // tokio built-in version instead). The compat layer fixes that.
                let read_compat: Compat<ReadHalf<SslStream<TcpStream>>> =
                    tokio_util::compat::TokioAsyncReadCompatExt::compat(read_half);

                let frame_reader: IncomingFrameReader =
                    FrameReader::new(LengthDelimited, read_compat, MESSAGE_FRAGMENT_SIZE);

                let carrier = Arc::new(Mutex::new(Demultiplexer::new(frame_reader)));

                // Now we can start the message reader.
                let boxed_span = Box::new(span.clone());
                effects.extend(
                    tasks::multi_channel_message_receiver(
                        self.context.clone(),
                        carrier,
                        ack_carrier,
                        self.incoming_limiter
                            .create_handle(peer_id, peer_consensus_public_key),
                        self.shutdown_fuse.inner().clone(),
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
        result: Result<(), MessageReaderError>,
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
                peer_consensus_public_key,
                transport,
            } => {
                info!("new outgoing connection established");

                let (senders, receivers) = unbounded_channels::<_, { Channel::COUNT }>();

                let handle = OutgoingHandle { senders, peer_addr };

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

                // `rust-openssl` does not support the futures 0.3 `AsyncWrite` trait (it uses the
                // tokio built-in version instead). The compat layer fixes that.

                let (read_half, write_half) = tokio::io::split(transport);

                let read_compat = tokio_util::compat::TokioAsyncReadCompatExt::compat(read_half);

                let ack_reader: AckFrameReader =
                    FrameReader::new(FixedSize::new(ACK_FRAME_SIZE), read_compat, ACK_BUFFER_SIZE);
                let ack_carrier = Arc::new(Mutex::new(Demultiplexer::new(ack_reader)));

                let write_compat =
                    tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(write_half);
                let carrier: OutgoingCarrier =
                    Multiplexer::new(FrameWriter::new(LengthDelimited, write_compat));

                effects.extend(
                    tasks::encoded_message_sender(
                        receivers,
                        carrier,
                        ack_carrier,
                        self.outgoing_limiter
                            .create_handle(peer_id, peer_consensus_public_key),
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

    fn handle_network_request(
        &self,
        request: NetworkRequest<P>,
        rng: &mut NodeRng,
    ) -> Effects<Event<P>> {
        match request {
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

        self.outgoing_limiter.remove_connected_validator(&peer_id);

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
        REv: FromIncoming<P> + From<PeerBehaviorAnnouncement>,
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
                    span,
                } => self.handle_incoming_closed(result, peer_id, peer_addr, *span),
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

/// Setup a fixed amount of senders/receivers.
fn unbounded_channels<T, const N: usize>() -> ([UnboundedSender<T>; N], [UnboundedReceiver<T>; N]) {
    // TODO: Improve this somehow to avoid the extra allocation required (turning a
    //       `Vec` into a fixed size array).
    let mut senders_vec = Vec::with_capacity(Channel::COUNT);

    let receivers: [_; N] = array_init(|_| {
        let (sender, receiver) = mpsc::unbounded_channel();
        senders_vec.push(sender);

        receiver
    });

    let senders: [_; N] = senders_vec
        .try_into()
        .expect("constant size array conversion failed");

    (senders, receivers)
}

/// Transport type for base encrypted connections.
type Transport = SslStream<TcpStream>;

/// The writer for outgoing length-prefixed frames.
type OutgoingFrameWriter = FrameWriter<
    ChannelPrefixedFrame<SingleFragment>,
    LengthDelimited,
    Compat<WriteHalf<Transport>>,
>;

/// The multiplexer to send fragments over an underlying frame writer.
type OutgoingCarrier = Multiplexer<OutgoingFrameWriter>;

/// The error type associated with the primary sink implementation.
type OutgoingChannelError =
    BackpressuredSinkError<MultiplexerError<io::Error>, DecodeError<DemultiplexerError<io::Error>>>;

/// An instance of a channel on an outgoing carrier.
type OutgoingChannel = BackpressuredSink<
    Fragmentizer<MultiplexerHandle<OutgoingFrameWriter>, Bytes>,
    IncomingAckChannel,
    Bytes,
>;

/// The reader for incoming length-prefixed frames.
type IncomingFrameReader = FrameReader<LengthDelimited, Compat<ReadHalf<Transport>>>;

/// The demultiplexer that seperates channels sent through the underlying frame reader.
type IncomingCarrier = Demultiplexer<IncomingFrameReader>;

/// An instance of a channel on an incoming carrier.
type IncomingChannel = BackpressuredStream<
    Defragmentizer<DemultiplexerHandle<IncomingFrameReader>>,
    OutgoingAckChannel,
    Bytes,
>;

/// Frame writer for ACKs, sent back over the incoming connection.
type AckFrameWriter =
    FrameWriter<ChannelPrefixedFrame<ImmediateFrameU64>, FixedSize, Compat<WriteHalf<Transport>>>;

/// ACK frames are 9 bytes (channel prefix + `u64`).
const ACK_FRAME_SIZE: usize = 9;

/// Frame reader for ACKs, received through an outgoing connection.
type AckFrameReader = FrameReader<FixedSize, Compat<ReadHalf<Transport>>>;

/// Multiplexer sending ACKs for various channels over an `AckFrameWriter`.
type OutgoingAckCarrier = Multiplexer<AckFrameWriter>;

/// Outgoing ACK sink.
type OutgoingAckChannel = LittleEndian<u64, MultiplexerHandle<AckFrameWriter>>;

/// Demultiplexer receiving ACKs for various channels over an `AckFrameReader`.
type IncomingAckCarrier = Demultiplexer<AckFrameReader>;

/// Incoming ACK stream.
type IncomingAckChannel = LittleEndian<u64, DemultiplexerHandle<AckFrameReader>>;

/// Setups bincode encoding used on the networking transport.
fn bincode_config() -> impl Options {
    bincode::options()
        .with_no_limit() // We rely on `muxink` to impose limits.
        .with_little_endian() // Default at the time of this writing, we are merely pinning it.
        .with_varint_encoding() // Same as above.
        .reject_trailing_bytes() // There is no reason for us not to reject trailing bytes.
}

/// Serializes a network message with the protocol specified encoding.
///
/// This function exists as a convenience, because there never should be a failure in serializing
/// messages we produced ourselves.
fn serialize_network_message<P>(msg: &Message<P>) -> Option<Bytes>
where
    P: Payload,
{
    bincode_config()
        .serialize(&msg)
        .map(Bytes::from)
        .map_err(|err| {
            error!(?msg, %err, "serialization failure when encoding outgoing message");
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
