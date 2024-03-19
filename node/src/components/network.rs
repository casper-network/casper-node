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
//! maintain a connection to each other node, see the [`conman`] module for details.
//!
//! Nodes gossip their public listening addresses periodically, and will try to establish and
//! maintain an outgoing connection to any new address learned.

pub(crate) mod blocklist;
mod chain_info;
mod config;
mod conman;
mod connection_id;
mod error;
mod event;
mod gossiped_address;
mod handshake;
mod identity;
mod insights;
mod message;
mod metrics;
mod per_channel;

#[cfg(test)]
mod tests;
mod transport;

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Debug,
    fs::OpenOptions,
    marker::PhantomData,
    mem,
    net::{SocketAddr, TcpListener},
    ops::Deref,
    sync::Arc,
    time::{Duration, Instant},
};

use bincode::Options;
use bytes::Bytes;
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};

use itertools::Itertools;
use juliet::rpc::{JulietRpcClient, RequestGuard};
use prometheus::Registry;
use rand::{
    seq::{IteratorRandom, SliceRandom},
    Rng,
};
use serde::Serialize;
use strum::EnumCount;
use tokio::net::TcpStream;
use tokio_openssl::SslStream;
use tracing::{debug, error, info, warn, Span};

use casper_types::{EraId, PublicKey, SecretKey};

use self::{
    chain_info::ChainInfo,
    conman::{ConMan, ConManState},
    handshake::HandshakeConfiguration,
    message::NodeKeyPair,
    metrics::Metrics,
    transport::TransportHandler,
};
pub(crate) use self::{
    config::Config,
    error::Error,
    event::Event,
    gossiped_address::GossipedAddress,
    identity::Identity,
    insights::NetworkInsights,
    message::{Channel, FromIncoming, Message, MessageKind, Payload},
    per_channel::PerChannel,
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
        self, display_error, rate_limited::rate_limited, DropSwitch, Fuse, LockedLineWriter,
        ObservableFuse, Source,
    },
    NodeRng,
};

use super::ValidatorBoundComponent;

/// The name of this component.
const COMPONENT_NAME: &str = "network";

/// How often to attempt to drop metrics, so that they can be re-registered.
const MAX_METRICS_DROP_ATTEMPTS: usize = 25;

/// Delays in between dropping metrics.
const DROP_RETRY_DELAY: Duration = Duration::from_millis(100);

/// How often metrics are synced.
const METRICS_UPDATE_RATE: Duration = Duration::from_secs(1);

#[derive(DataSize, Debug)]
pub(crate) struct Network<P>
where
    P: Payload,
{
    /// Initial configuration values.
    config: Config,
    /// The network address the component is listening on.
    ///
    /// Will be initialized late.
    public_addr: Option<SocketAddr>,
    /// Chain information used by networking.
    ///
    /// Only available during initialization.
    chain_info: ChainInfo,
    /// Consensus keys, used for handshaking.
    ///
    /// Only available during initialization.
    node_key_pair: Option<NodeKeyPair>,
    /// Node's network identify.
    identity: Identity,
    /// Our node identity. Derived from `identity`, cached here.
    our_id: NodeId,
    /// The set of known addresses that are eternally kept.
    known_addresses: HashSet<SocketAddr>,
    /// A reference to the global validator matrix.
    validator_matrix: ValidatorMatrix,
    /// Connection manager for incoming and outgoing connections.
    #[data_size(skip)] // Skipped, to reduce lock contention.
    conman: Option<ConMan>,
    /// Fuse signaling a shutdown of the small network.
    shutdown_fuse: DropSwitch<ObservableFuse>,
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

impl<P> Network<P>
where
    P: Payload,
{
    /// Creates a new network component instance.
    #[allow(clippy::type_complexity)]
    pub(crate) fn new<C: Into<ChainInfo>>(
        config: Config,
        identity: Identity,
        node_key_pair: Option<(Arc<SecretKey>, PublicKey)>,
        registry: &Registry,
        chain_info_source: C,
        validator_matrix: ValidatorMatrix,
    ) -> Result<Network<P>, Error> {
        let net_metrics = Arc::new(Metrics::new(registry)?);

        let node_key_pair = node_key_pair.map(NodeKeyPair::new);
        let our_id = identity.node_id();

        Ok(Network {
            config,
            known_addresses: Default::default(),
            public_addr: None,
            chain_info: chain_info_source.into(),
            node_key_pair: node_key_pair,
            identity,
            our_id,
            validator_matrix,
            conman: None,
            net_metrics,
            // We start with an empty set of validators for era 0 and expect to be updated.
            active_era: EraId::new(0),
            state: ComponentState::Uninitialized,
            shutdown_fuse: DropSwitch::new(ObservableFuse::new()),

            _payload: PhantomData,
        })
    }

    /// Initializes the networking component.
    fn initialize<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Event<P>>, Error>
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
        // Start by resolving all known addresses.
        let known_addresses =
            resolve_addresses(self.config.known_addresses.iter().map(String::as_str));

        // Assert we have at least one known address in the config.
        if known_addresses.is_empty() {
            warn!("no known addresses provided via config or all failed DNS resolution");
            return Err(Error::EmptyKnownHosts);
        }
        self.known_addresses = known_addresses;

        let mut public_addr =
            utils::resolve_address(&self.config.public_address).map_err(Error::ResolveAddr)?;

        // We can now create a listener.
        let bind_address =
            utils::resolve_address(&self.config.bind_address).map_err(Error::ResolveAddr)?;
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
        self.public_addr = Some(public_addr);

        let mut effects = Effects::new();

        // Start broadcasting our public listening address.
        effects.extend(
            effect_builder
                .set_timeout(self.config.initial_gossip_delay.into())
                .event(|_| Event::GossipOurAddress),
        );

        effects.extend(effect_builder.immediately().event(|_| Event::SyncMetrics));

        let keylog = match self.config.keylog_path {
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

        // Start connection manager.
        let rpc_builder = transport::create_rpc_builder(
            &self.chain_info.networking_config,
            &self.config,
            &self.chain_info,
        );

        // Setup connection manager, then learn all known addresses.
        let handshake_configuration = HandshakeConfiguration::new(
            self.chain_info.clone(),
            self.node_key_pair.clone(),
            public_addr,
        );

        let protocol_handler = TransportHandler::new(
            effect_builder.into_inner(),
            self.identity.clone(),
            handshake_configuration,
            keylog,
        );

        let conman = ConMan::new(
            tokio::net::TcpListener::from_std(listener).expect("not in tokio runtime"),
            public_addr,
            self.our_id,
            Box::new(protocol_handler),
            rpc_builder,
            self.config.conman,
        );
        self.conman = Some(conman);
        self.learn_known_addresses();

        // Done, set initialized state.
        <Self as InitializedComponent<REv>>::set_state(self, ComponentState::Initialized);

        Ok(effects)
    }

    /// Submits all known addresses to the connection manager.
    fn learn_known_addresses(&self) {
        let Some(ref conman) = self.conman else {
            error!("cannot learn known addresses, component not initialized");
            return;
        };

        for known_address in &self.known_addresses {
            conman.learn_addr(*known_address);
        }
    }

    /// Queues a message to be sent to validator nodes in the given era.
    fn broadcast_message_to_validators(&self, channel: Channel, payload: Bytes, _era_id: EraId) {
        let Some(ref conman) = self.conman else {
            error!(
                "cannot broadcast message to validators on non-initialized networking component"
            );
            return;
        };

        self.net_metrics.broadcast_requests.inc();

        // Determine whether we should restrict broadcasts at all.
        let validators = self.validator_matrix.active_or_upcoming_validators();
        if self.config.use_validator_broadcast && !validators.is_empty() {
            let state = conman.read_state();
            for (consensus_key, &peer_id) in state.key_index().iter() {
                if validators.contains(consensus_key) {
                    self.send_message(&*state, peer_id, channel, payload.clone(), None)
                }
            }
        } else {
            // We were asked to not use validator broadcasting, or do not have a list of validators
            // available. Broadcast to everyone instead.
            let state = conman.read_state();
            for &peer_id in state.routing_table().keys() {
                self.send_message(&*state, peer_id, channel, payload.clone(), None)
            }
        }
    }

    /// Queues a message to `count` random nodes on the network.
    ///
    /// Returns the IDs of the nodes the message has been gossiped to.
    fn gossip_message(
        &self,
        rng: &mut NodeRng,
        channel: Channel,
        payload: Bytes,
        gossip_target: GossipTarget,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let Some(ref conman) = self.conman else {
            error!("should never attempt to gossip on unintialized component");
            return Default::default();
        };
        let state = conman.read_state();

        // Collect all connected peers sans exclusion list.
        let connected_peers: Vec<_> = state
            .routing_table()
            .keys()
            .filter(|node_id| !exclude.contains(node_id))
            .collect();

        let mut chosen: Vec<NodeId> = match gossip_target {
            GossipTarget::Mixed(era_id) => {
                if let Some(known_era_validators) = self.validator_matrix.era_validators(era_id) {
                    // We have the validators for the given era by consensus key, map to node ID.
                    let connected_era_validators: HashSet<NodeId> = known_era_validators
                        .iter()
                        .filter_map(|key| state.key_index().get(key))
                        .filter(|node_id| !exclude.contains(node_id))
                        .cloned()
                        .collect();

                    // Create two separate batches, first all non-validators, second all validators.
                    let mut first = connected_peers
                        .iter()
                        .filter(|&node_id| !connected_era_validators.contains(node_id))
                        .map(Deref::deref)
                        .choose_multiple(rng, count);

                    let mut second = connected_era_validators.iter().choose_multiple(rng, count);

                    // Shuffle, then sample.
                    if rng.gen() {
                        mem::swap(&mut first, &mut second);
                    }
                    first.shuffle(rng);
                    second.shuffle(rng);

                    first
                        .into_iter()
                        .interleave(second.into_iter())
                        .take(count)
                        .cloned()
                        .collect()
                } else {
                    rate_limited!(
                        ERA_NOT_READY,
                        5,
                        Duration::from_secs(10),
                        |dropped| warn!(%gossip_target, dropped, "failed to select mixed target for era gossip")
                    );

                    // Fall through, keeping `chosen` empty.
                    Vec::new()
                }
            }
            GossipTarget::All => {
                // Simply fall through, since `GossipTarget::All` is also our fallback mode.
                Vec::new()
            }
        };

        if chosen.is_empty() {
            chosen.extend(connected_peers.choose_multiple(rng, count).cloned());
        }

        if chosen.len() != count {
            rate_limited!(
                GOSSIP_SELECTION_FELL_SHORT,
                5,
                Duration::from_secs(60),
                |dropped| warn!(%gossip_target, wanted=count, got=chosen.len(), dropped, "gossip selection fell short")
            );
        }

        for &peer_id in &chosen {
            self.send_message(&state, peer_id, channel, payload.clone(), None);
        }

        // TODO: We should actually return just the Vec instead.
        chosen.into_iter().collect()
    }

    /// Queues a message to be sent to a specific node.
    fn send_message(
        &self,
        state: &ConManState,
        dest: NodeId,
        channel: Channel,
        payload: Bytes,
        message_queued_responder: Option<AutoClosingResponder<()>>,
    ) {
        // Try to send the message.
        if let Some(route) = state.routing_table().get(&dest) {
            /// Build the request.
            ///
            /// Internal helper function to ensure requests are always built the same way.
            // Note: Ideally, this would be a closure, but lifetime inference does not
            //       work out here, and we cannot annotate lifetimes on closures.
            #[inline(always)]
            fn mk_request(
                rpc_client: &JulietRpcClient<{ Channel::COUNT }>,
                channel: Channel,
                payload: Bytes,
            ) -> juliet::rpc::JulietRpcRequestBuilder<'_, { Channel::COUNT }> {
                rpc_client
                    .create_request(channel.into_channel_id())
                    .with_payload(payload)
            }
            let request = mk_request(&route.client, channel, payload);

            // Attempt to enqueue it directly, regardless of what `message_queued_responder` is.
            match request.try_queue_for_sending() {
                Ok(guard) => process_request_guard(channel, guard),
                Err(builder) => {
                    // Failed to queue immediately, our next step depends on whether we were asked
                    // to keep trying or to discard.

                    // Reconstruct the payload.
                    let payload = match builder.into_payload() {
                        None => {
                            // This should never happen.
                            error!("payload unexpectedly disappeard");
                            return;
                        }
                        Some(payload) => payload,
                    };

                    if let Some(responder) = message_queued_responder {
                        // Reconstruct the client.
                        let client = route.client.clone();

                        // Technically, the queueing future should be spawned by the reactor, but
                        // since the networking component usually controls its own futures, we are
                        // allowed to spawn these as well.
                        tokio::spawn(async move {
                            let guard = mk_request(&client, channel, payload)
                                .queue_for_sending()
                                .await;
                            responder.respond(()).await;

                            // We need to properly process the guard, so it does not cause a
                            // cancellation from being dropped.
                            process_request_guard(channel, guard)
                        });
                    } else {
                        // We had to drop the message, since we hit the buffer limit.
                        match deserialize_network_message::<P>(&payload) {
                            Ok(reconstructed_message) => {
                                debug!(our_id=%self.our_id, %dest, msg=%reconstructed_message, "dropped outgoing message, buffer exhausted");
                            }
                            Err(err) => {
                                error!(our_id=%self.our_id,
                                       %dest,
                                       reconstruction_error=%err,
                                       ?payload,
                                       "dropped outgoing message, buffer exhausted and also failed to reconstruct it"
                                );
                            }
                        }

                        rate_limited!(
                            MESSAGE_RATE_EXCEEDED,
                            1,
                            Duration::from_secs(5),
                            |dropped| warn!(%channel, payload_len=payload.len(), dropped, "node is sending at too high a rate, message dropped")
                        );
                    }
                }
            }
        } else {
            rate_limited!(
                LOST_MESSAGE,
                5,
                Duration::from_secs(30),
                |dropped| warn!(%channel, %dest, size=payload.len(), dropped, "discarding message to peer, no longer connected")
            );
        }
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
                let Some(ref conman) = self.conman else {
                    error!("cannot send message on non-initialized network component");

                    return Effects::new();
                };

                let Some((channel, payload)) = stuff_into_envelope(*payload) else {
                    return Effects::new();
                };

                self.net_metrics.direct_message_requests.inc();

                // We're given a message to send. Pass on the responder so that confirmation
                // can later be given once the message has actually been buffered.
                self.send_message(
                    &*conman.read_state(),
                    *dest,
                    channel,
                    payload,
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
                let Some((channel, payload)) = stuff_into_envelope(*payload) else {
                    return Effects::new();
                };

                self.broadcast_message_to_validators(channel, payload, era_id);

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
                let Some((channel, payload)) = stuff_into_envelope(*payload) else {
                    return Effects::new();
                };

                let sent_to =
                    self.gossip_message(rng, channel, payload, gossip_target, count, exclude);

                auto_closing_responder.respond(sent_to).ignore()
            }
        }
    }

    /// Handles a received message.
    fn handle_incoming_message<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
        msg: Message<P>,
        ticket: Ticket,
        span: Span,
    ) -> Effects<Event<P>>
    where
        REv: FromIncoming<P> + From<NetworkRequest<P>> + From<PeerBehaviorAnnouncement> + Send,
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
            Message::Payload(payload) => effect_builder
                .announce_incoming(peer_id, payload, ticket)
                .ignore(),
        })
    }

    /// Returns the set of connected nodes.
    pub(crate) fn peers(&self) -> BTreeMap<NodeId, SocketAddr> {
        let Some(ref conman) = self.conman else {
            // Not initialized means no peers.
            return Default::default();
        };

        conman
            .read_state()
            .routing_table()
            .values()
            .map(|route| (route.peer, route.remote_addr))
            .collect()
    }

    /// Get a randomly sampled subset of connected peers
    pub(crate) fn connected_peers_random(&self, rng: &mut NodeRng, count: usize) -> Vec<NodeId> {
        let Some(ref conman) = self.conman else {
            // If we are not initialized, return an empty set.
            return Vec::new();
        };

        // Note: This is not ideal, since it os O(n) (n = number of peers), whereas for a slice it
        //       would be O(k) (k = number of items). If this proves to be a bottleneck, add an
        //       unstable `Vec` (allows O(1) random removal) to `ConMan` that stores a list of
        //       currently connected nodes.

        let mut subset = conman
            .read_state()
            .routing_table()
            .values()
            .map(|route| route.peer)
            .choose_multiple(rng, count);

        // Documentation says result must be shuffled to be truly random.
        subset.shuffle(rng);

        subset
    }

    /// Returns whether or not the threshold has been crossed for the component to consider itself
    /// sufficiently connected.
    pub(crate) fn has_sufficient_connected_peers(&self) -> bool {
        let Some(ref conman) = self.conman else {
            // If we are not initialized, we do not have any fully connected peers.
            return false;
        };

        let connection_count = conman.read_state().routing_table().len();
        connection_count >= self.config.min_peers_for_initialization as usize
    }

    #[cfg(test)]
    /// Returns the node id of this network node.
    pub(crate) fn node_id(&self) -> NodeId {
        self.our_id
    }
}

impl<P> Finalize for Network<P>
where
    P: Payload,
{
    fn finalize(self) -> BoxFuture<'static, ()> {
        async move {
            self.shutdown_fuse.inner().set();

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

fn resolve_addresses<'a>(addresses: impl Iterator<Item = &'a str>) -> HashSet<SocketAddr> {
    let mut resolved = HashSet::new();
    for address in addresses {
        match utils::resolve_address(address) {
            Ok(addr) => {
                if !resolved.insert(addr) {
                    warn!(%address, resolved=%addr, "ignoring duplicated address");
                };
            }
            Err(ref err) => {
                warn!(%address, err=display_error(err), "failed to resolve address");
            }
        }
    }
    resolved
}

impl<REv, P> Component<REv> for Network<P>
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
                Event::IncomingMessage { .. }
                | Event::NetworkRequest { .. }
                | Event::NetworkInfoRequest { .. }
                | Event::GossipOurAddress
                | Event::SyncMetrics
                | Event::PeerAddressReceived(_)
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
                Event::IncomingMessage {
                    peer_id,
                    msg,
                    span,
                    ticket,
                } => self.handle_incoming_message(effect_builder, *peer_id, *msg, ticket, span),
                Event::NetworkRequest { req: request } => {
                    self.handle_network_request(*request, rng)
                }
                Event::NetworkInfoRequest { req } => match *req {
                    NetworkInfoRequest::Peers { responder } => {
                        responder.respond(self.peers()).ignore()
                    }
                    NetworkInfoRequest::FullyConnectedPeers { count, responder } => responder
                        .respond(self.connected_peers_random(rng, count))
                        .ignore(),
                    NetworkInfoRequest::Insight { responder } => responder
                        .respond(NetworkInsights::collect_from_component(self))
                        .ignore(),
                },
                Event::GossipOurAddress => {
                    let mut effects = effect_builder
                        .set_timeout(self.config.gossip_interval.into())
                        .event(|_| Event::GossipOurAddress);

                    if let Some(public_address) = self.public_addr {
                        let our_address = GossipedAddress::new(public_address);
                        debug!( %our_address, "gossiping our addresses" );
                        effects.extend(
                            effect_builder
                                .begin_gossip(
                                    our_address,
                                    Source::Ourself,
                                    our_address.gossip_target(),
                                )
                                .ignore(),
                        );
                    } else {
                        // The address should have been set before we first trigger the gossiping,
                        // thus we should never end up here.
                        error!("cannot gossip our address, it is missing");
                    };

                    // We also ensure we know our known addresses still.
                    debug!(
                        address_count = self.known_addresses.len(),
                        "learning known addresses"
                    );
                    self.learn_known_addresses();

                    effects
                }
                Event::SyncMetrics => {
                    // Update the `peers` metric.
                    // TODO: Add additional metrics for bans, do-not-calls, etc.
                    let peers = if let Some(ref conman) = self.conman {
                        conman.read_state().routing_table().len()
                    } else {
                        0
                    };
                    self.net_metrics.peers.set(peers as i64);
                    effect_builder
                        .set_timeout(METRICS_UPDATE_RATE)
                        .event(|_| Event::SyncMetrics)
                }
                Event::PeerAddressReceived(gossiped_address) => {
                    if let Some(ref conman) = self.conman {
                        conman.learn_addr(gossiped_address.into());
                    } else {
                        error!("received gossiped address while component was not initialized");
                    }

                    Effects::new()
                }
                Event::BlocklistAnnouncement(announcement) => match announcement {
                    PeerBehaviorAnnouncement::OffenseCommitted {
                        offender,
                        justification,
                    } => {
                        if let Some(ref conman) = self.conman {
                            let now = Instant::now();
                            let until = now
                                + Duration::from_millis(
                                    self.config.blocklist_retain_duration.millis(),
                                );

                            conman.ban_peer(*offender, *justification, now, until);
                        } else {
                            error!("cannot ban, component not initialized");
                        };

                        Effects::new()
                    }
                },
            },
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv, P> InitializedComponent<REv> for Network<P>
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

impl<REv, P> ValidatorBoundComponent<REv> for Network<P>
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

        let _active_validators = self.validator_matrix.active_or_upcoming_validators();

        // Update the validator status for every connection.
        // for (public_key, status) in self.incoming_validator_status.iter_mut() {
        //     // If there is only a `Weak` ref, we lost the connection to the validator, but the
        //     // disconnection has not reached us yet.
        //     if let Some(arc) = status.upgrade() {
        //         arc.store(
        //             active_validators.contains(public_key),
        //             std::sync::atomic::Ordering::Relaxed,
        //         )
        //     }
        // }
        // TODO: Restore functionality.

        Effects::default()
    }
}

/// Transport type for base encrypted connections.
type Transport = SslStream<TcpStream>;

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

/// Given a message payload, puts it into a proper message envelope and returns the serialized
/// envelope along with the channel it should be sent on.
#[inline(always)]
fn stuff_into_envelope<P: Payload>(payload: P) -> Option<(Channel, Bytes)> {
    let msg = Message::Payload(payload);
    let channel = msg.get_channel();
    let byte_payload = serialize_network_message(&msg)?;
    Some((channel, byte_payload))
}

/// Deserializes a networking message from the protocol specified encoding.
fn deserialize_network_message<P>(bytes: &[u8]) -> Result<Message<P>, bincode::Error>
where
    P: Payload,
{
    bincode_config().deserialize(bytes)
}

/// Processes a request guard obtained by making a request to a peer through Juliet RPC.
///
/// Ensures that outgoing messages are not cancelled, a would be the case when simply dropping the
/// `RequestGuard`. Potential errors that are available early are dropped, later errors discarded.
#[inline]
fn process_request_guard(channel: Channel, guard: RequestGuard) {
    match guard.try_get_response() {
        Ok(Ok(_outcome)) => {
            // We got an incredibly quick round-trip, lucky us! Nothing to do.
        }
        Ok(Err(err)) => {
            rate_limited!(
                MESSAGE_SENDING_FAILURE,
                5,
                Duration::from_secs(60),
                |dropped| warn!(%channel, %err, dropped, "failed to send message")
            );
        }
        Err(guard) => {
            // No ACK received yet, forget, so we don't cancel.
            guard.forget();
        }
    }
}
