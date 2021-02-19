mod behavior;
mod config;
mod error;
mod event;
mod gossip;
mod one_way_messaging;
mod peer_discovery;
mod protocol_id;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bulk_gossip;

use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::{self, Debug, Display, Formatter},
    marker::PhantomData,
    num::NonZeroU32,
    sync::{Arc, Mutex},
    time::Duration,
};

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use libp2p::{
    core::{connection::ConnectedPoint, upgrade},
    gossipsub::GossipsubEvent,
    identify::IdentifyEvent,
    identity::Keypair,
    kad::KademliaEvent,
    mplex::{MaxBufferBehaviour, MplexConfig},
    noise::{self, NoiseConfig, X25519Spec},
    request_response::{RequestResponseEvent, RequestResponseMessage},
    swarm::{SwarmBuilder, SwarmEvent},
    tcp::TokioTcpConfig,
    Multiaddr, PeerId, Swarm, Transport,
};
use prometheus::{IntGauge, Registry};
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use tokio::{select, sync::watch, task::JoinHandle};
use tracing::{debug, error, info, trace, warn};

pub(crate) use self::event::Event;
use self::{
    behavior::{Behavior, SwarmBehaviorEvent},
    gossip::GossipMessage,
    one_way_messaging::{Codec as OneWayCodec, Outgoing as OneWayOutgoingMessage},
    protocol_id::ProtocolId,
};
pub use self::{config::Config, error::Error};
use crate::{
    components::{networking_metrics::NetworkingMetrics, Component},
    effect::{
        announcements::NetworkAnnouncement,
        requests::{NetworkInfoRequest, NetworkRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    reactor::{EventQueueHandle, Finalize, QueueKind},
    types::{Chainspec, NodeId},
    utils::{counting_unbounded_channel, ds, CountingReceiver, CountingSender, DisplayIter},
    NodeRng,
};

/// Env var which, if it's defined at runtime, enables the network (libp2p based) component.
pub(crate) const ENABLE_LIBP2P_NET_ENV_VAR: &str = "CASPER_ENABLE_LIBP2P_NET";

/// How long to sleep before reconnecting
const RECONNECT_DELAY: Duration = Duration::from_millis(500);

/// A helper trait whose bounds represent the requirements for a payload that `Network` can
/// work with.
pub trait PayloadT:
    Serialize + for<'de> Deserialize<'de> + Clone + Debug + Display + Send + 'static
{
}

impl<P> PayloadT for P where
    P: Serialize + for<'de> Deserialize<'de> + Clone + Debug + Display + Send + 'static
{
}

/// A helper trait whose bounds represent the requirements for a reactor event that `Network` can
/// work with.
pub trait ReactorEventT<P: PayloadT>:
    From<Event<P>> + From<NetworkAnnouncement<NodeId, P>> + Send + 'static
{
}

impl<REv, P> ReactorEventT<P> for REv
where
    P: PayloadT,
    REv: From<Event<P>> + From<NetworkAnnouncement<NodeId, P>> + Send + 'static,
{
}

#[derive(PartialEq, Eq, Debug, DataSize)]
enum ConnectionState {
    Pending,
    Connected,
    Failed,
}

// TODO: Get rid of the `Arc<Mutex<_>>` ASAP.
fn estimate_known_addresses(map: &Arc<Mutex<HashMap<Multiaddr, ConnectionState>>>) -> usize {
    ds::hash_map_fixed_size(&*(map.lock().expect("lock poisoned")))
}

#[derive(DataSize)]
pub struct Network<REv, P> {
    #[data_size(skip)]
    network_identity: NetworkIdentity,
    our_id: NodeId,
    /// The set of peers which are current connected to our node. Kept in sync with libp2p
    /// internals.
    // DataSize note: Connected point contains `Arc`'ed Vecs internally, this is better than
    // skipping at least.
    #[data_size(with = ds::hash_map_fixed_size)]
    peers: HashMap<NodeId, ConnectedPoint>,
    /// The set of peers whose address we currently know. Kept in sync with the internal Kademlia
    /// routing table.
    // DataSize note: `PeerId`s can likely be estimated using `mem::size_of`.
    #[data_size(with = ds::hash_set_fixed_size)]
    seen_peers: HashSet<PeerId>,
    #[data_size(with = ds::vec_fixed_size)]
    listening_addresses: Vec<Multiaddr>,
    /// The addresses of known peers to be used for bootstrapping, and their connection states.
    /// Wrapped in a [Mutex] so it can be shared with [SwarmEvent] handling (which runs in a
    /// separate thread).
    #[data_size(with = estimate_known_addresses)]
    known_addresses_mut: Arc<Mutex<HashMap<Multiaddr, ConnectionState>>>,
    /// Whether this node is a bootstrap node or not.
    is_bootstrap_node: bool,
    /// The channel through which to send outgoing one-way requests.
    one_way_message_sender: CountingSender<OneWayOutgoingMessage>,
    max_one_way_message_size: u32,
    /// The channel through which to send new messages for gossiping.
    gossip_message_sender: CountingSender<GossipMessage>,
    max_gossip_message_size: u32,
    /// Channel signaling a shutdown of the network component.
    #[data_size(skip)]
    shutdown_sender: Option<watch::Sender<()>>,
    server_join_handle: Option<JoinHandle<()>>,

    /// Networking metrics.
    #[data_size(skip)]
    net_metrics: NetworkingMetrics,

    _phantom: PhantomData<(REv, P)>,
}

impl<REv: ReactorEventT<P>, P: PayloadT> Network<REv, P> {
    /// Creates a new small network component instance.
    ///
    /// If `notify` is set to `false`, no systemd notifications will be sent, regardless of
    /// configuration.
    #[allow(clippy::type_complexity)]
    pub(crate) fn new(
        event_queue: EventQueueHandle<REv>,
        config: Config,
        registry: &Registry,
        network_identity: NetworkIdentity,
        chainspec: &Chainspec,
        notify: bool,
    ) -> Result<(Network<REv, P>, Effects<Event<P>>), Error> {
        let our_peer_id = PeerId::from(&network_identity);
        let our_id = NodeId::from(&network_identity);

        // Convert the known addresses to multiaddr format and prepare the shutdown signal.
        let known_addresses = config
            .known_addresses
            .iter()
            .map(|address| {
                let multiaddr = address_str_to_multiaddr(address.as_str());
                (multiaddr, ConnectionState::Pending)
            })
            .collect::<HashMap<_, _>>();

        // Assert we have at least one known address in the config.
        if known_addresses.is_empty() {
            warn!("{}: no known addresses provided via config", our_id);
            return Err(Error::NoKnownAddress);
        }

        let (one_way_message_sender, one_way_message_receiver) = counting_unbounded_channel();
        let (gossip_message_sender, gossip_message_receiver) = counting_unbounded_channel();
        let (server_shutdown_sender, server_shutdown_receiver) = watch::channel(());

        // If the env var "CASPER_ENABLE_LIBP2P_NET" is defined, start the server and exit.
        if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_err() {
            let network = Network {
                network_identity,
                our_id,
                peers: HashMap::new(),
                seen_peers: HashSet::new(),
                listening_addresses: vec![],
                known_addresses_mut: Arc::new(Mutex::new(known_addresses)),
                is_bootstrap_node: config.is_bootstrap_node,
                one_way_message_sender,
                max_one_way_message_size: 0,
                gossip_message_sender,
                max_gossip_message_size: 0,
                shutdown_sender: Some(server_shutdown_sender),
                server_join_handle: None,
                net_metrics: NetworkingMetrics::new(&Registry::default())?,
                _phantom: PhantomData,
            };
            return Ok((network, Effects::new()));
        }

        let net_metrics = NetworkingMetrics::new(registry).map_err(Error::MetricsError)?;

        if notify {
            debug!("our node id: {}", our_id);
        }

        // Create a keypair for authenticated encryption of the transport.
        let noise_keys = noise::Keypair::<X25519Spec>::new()
            .into_authentic(&network_identity.keypair)
            .map_err(Error::StaticKeypairSigning)?;

        let mut mplex_config = MplexConfig::default();
        mplex_config.max_buffer_len_behaviour(MaxBufferBehaviour::Block);

        // Create a tokio-based TCP transport.  Use `noise` for authenticated encryption and `mplex`
        // for multiplexing of substreams on a TCP stream.
        let transport = TokioTcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(MplexConfig::default())
            .timeout(config.connection_setup_timeout.into())
            .boxed();

        // Create a Swarm to manage peers and events.
        let behavior = Behavior::new(
            &config,
            &net_metrics,
            chainspec,
            network_identity.keypair.public(),
        );
        let mut swarm = SwarmBuilder::new(transport, behavior, our_peer_id)
            .executor(Box::new(|future| {
                tokio::spawn(future);
            }))
            .build();

        // Specify listener.
        let listening_address = address_str_to_multiaddr(config.bind_address.as_str());
        Swarm::listen_on(&mut swarm, listening_address.clone()).map_err(|error| Error::Listen {
            address: listening_address.clone(),
            error,
        })?;
        info!(%our_id, %listening_address, "network component started listening");

        // Schedule connection attempts to known peers.
        for address in known_addresses.keys() {
            debug!(%our_id, %address, "dialing known address");
            Swarm::dial_addr(&mut swarm, address.clone()).map_err(|error| Error::DialPeer {
                address: address.clone(),
                error,
            })?;
        }

        // Wrap the known_addresses in a mutex so we can share it with the server task.
        let known_addresses_mut = Arc::new(Mutex::new(known_addresses));
        let is_bootstrap_node = config.is_bootstrap_node;

        // Start the server task.
        let server_join_handle = Some(tokio::spawn(server_task(
            event_queue,
            one_way_message_receiver,
            gossip_message_receiver,
            server_shutdown_receiver,
            swarm,
            known_addresses_mut.clone(),
            is_bootstrap_node,
            net_metrics.queued_messages.clone(),
        )));

        let network = Network {
            network_identity,
            our_id,
            peers: HashMap::new(),
            seen_peers: HashSet::new(),
            listening_addresses: vec![],
            known_addresses_mut,
            is_bootstrap_node,
            one_way_message_sender,
            max_one_way_message_size: config.max_one_way_message_size,
            gossip_message_sender,
            max_gossip_message_size: config.max_gossip_message_size,
            shutdown_sender: Some(server_shutdown_sender),
            server_join_handle,
            net_metrics,
            _phantom: PhantomData,
        };
        Ok((network, Effects::new()))
    }

    fn handle_connection_established(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
        endpoint: ConnectedPoint,
        num_established: NonZeroU32,
    ) -> Effects<Event<P>> {
        debug!(%peer_id, ?endpoint, %num_established,"{}: connection established", self.our_id);

        if let ConnectedPoint::Dialer { ref address } = endpoint {
            let mut known_addresses = match self.known_addresses_mut.lock() {
                Ok(known_addresses) => known_addresses,
                Err(err) => {
                    return fatal!(
                        effect_builder,
                        "Could not acquire `known_addresses_mut` mutex: {:?}",
                        err
                    )
                }
            };
            if let Some(state) = known_addresses.get_mut(address) {
                if *state == ConnectionState::Pending {
                    *state = ConnectionState::Connected
                }
            }
        };

        let _ = self.peers.insert(peer_id.clone(), endpoint);

        self.net_metrics.peers.set(self.peers.len() as i64);
        // TODO - see if this can be removed.  The announcement is only used by the joiner reactor.
        effect_builder.announce_new_peer(peer_id).ignore()
    }

    /// Queues a message to be sent to a specific node.
    fn send_message(&self, destination: NodeId, payload: P) {
        let outgoing_message = match OneWayOutgoingMessage::new(
            destination,
            &payload,
            self.max_one_way_message_size,
        ) {
            Ok(msg) => msg,
            Err(error) => {
                warn!(%error, %payload, "{}: failed to construct outgoing message", self.our_id);
                return;
            }
        };
        if let Err(error) = self.one_way_message_sender.send_datasized(outgoing_message) {
            warn!(%error, "{}: dropped outgoing message, server has shut down", self.our_id);
        } else {
            // `queued_message` might become -1 for a short amount of time, which is fine.
            self.net_metrics.queued_messages.inc();
        }
    }

    /// Queues a message to be sent to all nodes.
    fn gossip_message(&self, payload: P) {
        let gossip_message = match GossipMessage::new(&payload, self.max_gossip_message_size) {
            Ok(msg) => msg,
            Err(error) => {
                warn!(%error, %payload, "{}: failed to construct new gossip message", self.our_id);
                return;
            }
        };
        if let Err(error) = self.gossip_message_sender.send_datasized(gossip_message) {
            warn!(%error, "{}: dropped new gossip message, server has shut down", self.our_id);
        }
    }

    /// Queues a message to `count` random nodes on the network.
    fn send_message_to_n_peers(
        &self,
        rng: &mut NodeRng,
        payload: P,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId> {
        let peer_ids = self
            .peers
            .keys()
            .filter(|&peer_id| !exclude.contains(peer_id))
            .choose_multiple(rng, count);

        if peer_ids.len() != count {
            // TODO - set this to `warn!` once we are normally testing with networks large enough to
            //        make it a meaningful and infrequent log message.
            trace!(
                wanted = count,
                selected = peer_ids.len(),
                "{}: could not select enough random nodes for gossiping, not enough non-excluded \
                outgoing connections",
                self.our_id
            );
        }

        for &peer_id in &peer_ids {
            self.send_message(peer_id.clone(), payload.clone());
        }

        peer_ids.into_iter().cloned().collect()
    }

    /// Returns the node id of this network node.
    #[cfg(test)]
    pub(crate) fn node_id(&self) -> NodeId {
        self.our_id.clone()
    }

    /// Returns the set of known addresses.
    #[cfg(test)]
    pub(crate) fn seen_peers(&self) -> &HashSet<PeerId> {
        &self.seen_peers
    }
}

fn our_id(swarm: &Swarm<Behavior>) -> NodeId {
    NodeId::P2p(Swarm::local_peer_id(swarm).clone())
}

// TODO: Already refactored in branch.
#[allow(clippy::too_many_arguments)]
async fn server_task<REv: ReactorEventT<P>, P: PayloadT>(
    event_queue: EventQueueHandle<REv>,
    // Receives outgoing one-way messages to be sent out via libp2p.
    mut one_way_outgoing_message_receiver: CountingReceiver<OneWayOutgoingMessage>,
    // Receives new gossip messages to be sent out via libp2p.
    mut gossip_message_receiver: CountingReceiver<GossipMessage>,
    // Receives notification to shut down the server loop.
    mut shutdown_receiver: watch::Receiver<()>,
    mut swarm: Swarm<Behavior>,
    known_addresses_mut: Arc<Mutex<HashMap<Multiaddr, ConnectionState>>>,
    is_bootsrap_node: bool,
    queued_messages: IntGauge,
) {
    //let our_id = our
    async move {
        loop {
            // Note that `select!` will cancel all futures on branches not eventually selected by
            // dropping them.  Each future inside this macro must be cancellation-safe.
            select! {
                // `swarm.next_event()` is cancellation-safe - see
                // https://github.com/libp2p/rust-libp2p/issues/1876
                swarm_event = swarm.next_event() => {
                    trace!("{}: {:?}", our_id(&swarm), swarm_event);
                    handle_swarm_event(&mut swarm, event_queue, swarm_event, &known_addresses_mut, is_bootsrap_node).await;
                }

                // `UnboundedReceiver::recv()` is cancellation safe - see
                // https://tokio.rs/tokio/tutorial/select#cancellation
                maybe_outgoing_message = one_way_outgoing_message_receiver.recv() => {
                    match maybe_outgoing_message {
                        Some(outgoing_message) => {
                            queued_messages.dec();

                            // We've received a one-way request to send to a peer.
                            swarm.send_one_way_message(outgoing_message);
                        }
                        None => {
                            // The data sender has been dropped - exit the loop.
                            info!("{}: exiting network server task", our_id(&swarm));
                            break;
                        }
                    }
                }

                // `UnboundedReceiver::recv()` is cancellation safe - see
                // https://tokio.rs/tokio/tutorial/select#cancellation
                maybe_gossip_message = gossip_message_receiver.recv() => {
                    match maybe_gossip_message {
                        Some(gossip_message) => {
                            // We've received a new message to be gossiped.
                            swarm.gossip(gossip_message);
                        }
                        None => {
                            // The data sender has been dropped - exit the loop.
                            info!("{}: exiting network server task", our_id(&swarm));
                            break;
                        }
                    }
                }

                maybe_shutdown = shutdown_receiver.recv() => {
                    // Since a `watch` channel is always constructed with an initial value enqueued,
                    // ignore this (and any others) from the `shutdown_receiver`.
                    //
                    // When the receiver yields a `None`, the sender has been dropped, indicating we
                    // should exit this loop.
                    if maybe_shutdown.is_none() {
                        info!("{}: shutting down libp2p", our_id(&swarm));
                        break;
                    }
                }
            }
        }
    }
    .await;
}

async fn handle_swarm_event<REv: ReactorEventT<P>, P: PayloadT, E: Display>(
    swarm: &mut Swarm<Behavior>,
    event_queue: EventQueueHandle<REv>,
    swarm_event: SwarmEvent<SwarmBehaviorEvent, E>,
    known_addresses_mut: &Arc<Mutex<HashMap<Multiaddr, ConnectionState>>>,
    is_bootstrap_node: bool,
) {
    let event = match swarm_event {
        SwarmEvent::ConnectionEstablished {
            peer_id,
            endpoint,
            num_established,
        } => {
            // If we dialed the peer, add their listening address to our kademlia instance.
            if endpoint.is_dialer() {
                swarm.add_discovered_peer(&peer_id, vec![endpoint.get_remote_address().clone()]);
            }
            Event::ConnectionEstablished {
                peer_id: Box::new(NodeId::from(peer_id)),
                endpoint,
                num_established,
            }
        }
        SwarmEvent::ConnectionClosed {
            peer_id,
            endpoint,
            num_established,
            cause,
        } => {
            // If we lost the final connection to this peer, do a random kademlia lookup to
            // discover any new/replacement peers.
            if num_established == 0 {
                swarm.discover_peers()
            }
            Event::ConnectionClosed {
                peer_id: Box::new(NodeId::from(peer_id)),
                endpoint,
                num_established,
                cause: cause.map(|error| error.to_string()),
            }
        }
        SwarmEvent::UnreachableAddr {
            peer_id,
            address,
            error,
            attempts_remaining,
        } => Event::UnreachableAddress {
            peer_id: Box::new(NodeId::from(peer_id)),
            address,
            error,
            attempts_remaining,
        },
        SwarmEvent::UnknownPeerUnreachableAddr { address, error } => {
            debug!(%address, %error, "{}: failed to connect", our_id(&swarm));
            let we_are_isolated = match known_addresses_mut.lock() {
                Err(err) => {
                    panic!("Could not acquire `known_addresses_mut` mutex: {:?}", err)
                }
                Ok(mut known_addresses) => {
                    if let Some(state) = known_addresses.get_mut(&address) {
                        if *state == ConnectionState::Pending {
                            *state = ConnectionState::Failed
                        }
                    }
                    network_is_isolated(&*known_addresses)
                }
            };

            if we_are_isolated {
                if is_bootstrap_node {
                    info!(
                        "{}: failed to bootstrap to any other nodes, but continuing to run as we \
                             are a bootstrap node",
                        our_id(&swarm)
                    );
                } else {
                    // (Re)schedule connection attempts to known peers.

                    // Before reconnecting wait RECONNECT_DELAY
                    tokio::time::delay_for(RECONNECT_DELAY).await;

                    // Now that we've slept and re-awoken, grab the mutex again
                    match known_addresses_mut.lock() {
                        Err(err) => {
                            panic!("Could not acquire `known_addresses_mut` mutex: {:?}", err)
                        }
                        Ok(known_addresses) => {
                            for address in known_addresses.keys() {
                                let our_id = our_id(&swarm);
                                debug!(%our_id, %address, "dialing known address");
                                Swarm::dial_addr(swarm, address.clone()).unwrap_or_else(|err| {
                                    error!(%our_id, %address,
                                           "Swarm error when rescheduling connection: {:?}",
                                           err)
                                });
                            }
                        }
                    };
                }
            }
            return;
        }
        SwarmEvent::NewListenAddr(address) => Event::NewListenAddress(address),
        SwarmEvent::ExpiredListenAddr(address) => Event::ExpiredListenAddress(address),
        SwarmEvent::ListenerClosed { addresses, reason } => {
            Event::ListenerClosed { addresses, reason }
        }
        SwarmEvent::ListenerError { error } => Event::ListenerError { error },
        SwarmEvent::Behaviour(SwarmBehaviorEvent::OneWayMessaging(event)) => {
            return handle_one_way_messaging_event(swarm, event_queue, event).await;
        }
        SwarmEvent::Behaviour(SwarmBehaviorEvent::Gossiper(event)) => {
            return handle_gossip_event(swarm, event_queue, event).await;
        }
        SwarmEvent::Behaviour(SwarmBehaviorEvent::Kademlia(KademliaEvent::RoutingUpdated {
            peer,
            old_peer,
            ..
        })) => Event::RoutingTableUpdated { peer, old_peer },
        SwarmEvent::Behaviour(SwarmBehaviorEvent::Kademlia(event)) => {
            debug!(?event, "{}: new kademlia event", our_id(swarm));
            return;
        }
        SwarmEvent::Behaviour(SwarmBehaviorEvent::Identify(event)) => {
            return handle_identify_event(swarm, event);
        }
        SwarmEvent::IncomingConnection { .. }
        | SwarmEvent::IncomingConnectionError { .. }
        | SwarmEvent::BannedPeer { .. }
        | SwarmEvent::Dialing(_) => return,
    };
    event_queue.schedule(event, QueueKind::Network).await;
}

/// Takes the known_addresses of a node and returns if it is isolated.
///
/// An isolated node has no chance of recovering a connection to the network and is not
/// connected to any peer.
fn network_is_isolated(known_addresses: &HashMap<Multiaddr, ConnectionState>) -> bool {
    known_addresses
        .values()
        .all(|state| *state == ConnectionState::Failed)
}

async fn handle_one_way_messaging_event<REv: ReactorEventT<P>, P: PayloadT>(
    swarm: &mut Swarm<Behavior>,
    event_queue: EventQueueHandle<REv>,
    event: RequestResponseEvent<Vec<u8>, ()>,
) {
    match event {
        RequestResponseEvent::Message {
            peer,
            message: RequestResponseMessage::Request { request, .. },
        } => {
            // We've received a one-way request from a peer: announce it via the reactor on the
            // `NetworkIncoming` queue.
            let sender = NodeId::from(peer);
            match bincode::deserialize::<P>(&request) {
                Ok(payload) => {
                    debug!(%sender, %payload, "{}: incoming one-way message received", our_id(swarm));
                    event_queue
                        .schedule(
                            NetworkAnnouncement::MessageReceived { sender, payload },
                            QueueKind::NetworkIncoming,
                        )
                        .await;
                }
                Err(error) => {
                    warn!(
                        %sender,
                        %error,
                        "{}: failed to deserialize incoming one-way message",
                        our_id(swarm)
                    );
                }
            }
        }
        RequestResponseEvent::Message {
            message: RequestResponseMessage::Response { .. },
            ..
        } => {
            // Note that a response will still be emitted immediately after the request has been
            // sent, since `RequestResponseCodec::read_response` for the one-way Codec does not
            // actually read anything from the given I/O stream.
        }
        RequestResponseEvent::OutboundFailure {
            peer,
            request_id,
            error,
        } => {
            warn!(
                ?peer,
                ?request_id,
                ?error,
                "{}: outbound failure",
                our_id(swarm)
            )
        }
        RequestResponseEvent::InboundFailure {
            peer,
            request_id,
            error,
        } => {
            warn!(
                ?peer,
                ?request_id,
                ?error,
                "{}: inbound failure",
                our_id(swarm)
            )
        }
    }
}

async fn handle_gossip_event<REv: ReactorEventT<P>, P: PayloadT>(
    swarm: &mut Swarm<Behavior>,
    event_queue: EventQueueHandle<REv>,
    event: GossipsubEvent,
) {
    match event {
        GossipsubEvent::Message(_sender, _message_id, message) => {
            // We've received a gossiped message: announce it via the reactor on the
            // `NetworkIncoming` queue.
            let sender = match message.source {
                Some(source) => NodeId::from(source),
                None => {
                    warn!(%_sender, ?message, "{}: libp2p gossiped message without source", our_id(swarm));
                    return;
                }
            };
            match bincode::deserialize::<P>(&message.data) {
                Ok(payload) => {
                    debug!(%sender, %payload, "{}: libp2p gossiped message received", our_id(swarm));
                    event_queue
                        .schedule(
                            NetworkAnnouncement::MessageReceived { sender, payload },
                            QueueKind::NetworkIncoming,
                        )
                        .await;
                }
                Err(error) => {
                    warn!(
                        %sender,
                        %error,
                        "{}: failed to deserialize gossiped message",
                        our_id(swarm)
                    );
                }
            }
        }
        GossipsubEvent::Subscribed { peer_id, .. } => {
            debug!(%peer_id, "{}: new gossip subscriber", our_id(swarm));
        }
        GossipsubEvent::Unsubscribed { peer_id, .. } => {
            debug!(%peer_id, "{}: peer unsubscribed from gossip", our_id(swarm));
        }
    }
}

fn handle_identify_event(swarm: &mut Swarm<Behavior>, event: IdentifyEvent) {
    match event {
        IdentifyEvent::Received {
            peer_id,
            info,
            observed_addr,
        } => {
            debug!(
                %peer_id,
                %info.protocol_version,
                %info.agent_version,
                ?info.listen_addrs,
                ?info.protocols,
                %observed_addr,
                "{}: identifying info received",
                our_id(swarm)
            );
            // We've received identifying information from a peer, so add its listening addresses to
            // our kademlia instance.
            swarm.add_discovered_peer(&peer_id, info.listen_addrs);
        }
        IdentifyEvent::Sent { peer_id } => {
            debug!(
                "{}: sent our identifying info to {}",
                our_id(swarm),
                peer_id
            );
        }
        IdentifyEvent::Error { peer_id, error } => {
            warn!(%peer_id, %error, "{}: error while attempting to identify peer", our_id(swarm));
        }
    }
}

/// Converts a string of the form "127.0.0.1:34553" into a Multiaddr equivalent to
/// "/ip4/127.0.0.1/tcp/34553".
fn address_str_to_multiaddr(address: &str) -> Multiaddr {
    let mut parts_itr = address.split(':');
    let multiaddr_str = if address
        .chars()
        .next()
        .expect("cannot convert empty address")
        .is_numeric()
    {
        format!(
            "/ip4/{}/tcp/{}",
            parts_itr.next().expect("address should contain IP segment"),
            parts_itr
                .next()
                .expect("address should contain port segment")
        )
    } else {
        format!(
            "/dns/{}/tcp/{}",
            parts_itr
                .next()
                .expect("address should contain DNS name segment"),
            parts_itr
                .next()
                .expect("address should contain port segment")
        )
    };
    // OK to `expect` for now as this method will become redundant once small_network is removed.
    multiaddr_str
        .parse()
        .expect("address should parse as a multiaddr")
}

impl<REv: Send + 'static, P: Send + 'static> Finalize for Network<REv, P> {
    fn finalize(mut self) -> BoxFuture<'static, ()> {
        async move {
            // Close the shutdown socket, causing the server to exit.
            drop(self.shutdown_sender.take());

            // Wait for the server to exit cleanly.
            if let Some(join_handle) = self.server_join_handle.take() {
                match join_handle.await {
                    Ok(_) => debug!("{}: server exited cleanly", self.our_id),
                    Err(err) => error!(%err, "{}: could not join server task cleanly", self.our_id),
                }
            } else if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_ok() {
                warn!("{}: server shutdown while already shut down", self.our_id)
            }
        }
        .boxed()
    }
}

impl<REv, P> Debug for Network<REv, P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Network")
            .field("our_id", &self.our_id)
            .field("peers", &self.peers)
            .field("listening_addresses", &self.listening_addresses)
            .field("known_addresses", &self.known_addresses_mut)
            .finish()
    }
}

impl<REv: ReactorEventT<P>, P: PayloadT> Component<REv> for Network<REv, P> {
    type Event = Event<P>;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!("{}: {:?}", self.our_id, event);
        match event {
            Event::ConnectionEstablished {
                peer_id,
                endpoint,
                num_established,
            } => self.handle_connection_established(
                effect_builder,
                *peer_id,
                endpoint,
                num_established,
            ),
            Event::ConnectionClosed {
                peer_id,
                endpoint,
                num_established,
                cause,
            } => {
                if num_established == 0 {
                    let _ = self.peers.remove(&peer_id);
                }
                debug!(%peer_id, ?endpoint, %num_established, ?cause, "{}: connection closed", self.our_id);

                // Note: We count multiple connections to the same peer as a single connection.
                self.net_metrics.peers.set(self.peers.len() as i64);

                Effects::new()
            }
            Event::UnreachableAddress {
                peer_id,
                address,
                error,
                attempts_remaining,
            } => {
                debug!(%peer_id, %address, %error, %attempts_remaining, "{}: failed to connect", self.our_id);
                Effects::new()
            }
            Event::NewListenAddress(address) => {
                self.listening_addresses.push(address);
                info!(
                    "{}: listening on {}",
                    self.our_id,
                    DisplayIter::new(self.listening_addresses.iter())
                );
                Effects::new()
            }
            Event::ExpiredListenAddress(address) => {
                self.listening_addresses.retain(|addr| *addr != address);
                if self.listening_addresses.is_empty() {
                    return fatal!(effect_builder, "no remaining listening addresses");
                }
                debug!(%address, "{}: listening address expired", self.our_id);
                Effects::new()
            }
            Event::ListenerClosed { reason, .. } => {
                // If the listener closed without an error, we're already shutting down the server.
                // Otherwise, we need to kill the node as it cannot function without a listener.
                match reason {
                    Err(error) => fatal!(effect_builder, "listener closed: {}", error),
                    Ok(()) => {
                        debug!("{}: listener closed", self.our_id);
                        Effects::new()
                    }
                }
            }
            Event::ListenerError { error } => {
                debug!(%error, "{}: non-fatal listener error", self.our_id);
                Effects::new()
            }
            Event::RoutingTableUpdated { peer, old_peer } => {
                if let Some(ref old_peer_id) = old_peer {
                    self.seen_peers.remove(old_peer_id);
                }
                self.seen_peers.insert(peer.clone());

                debug!(
                    inserted = ?peer,
                    removed = ?old_peer,
                    new_size = self.seen_peers.len(),
                    "kademlia routing table updated"
                );

                Effects::new()
            }

            Event::NetworkRequest {
                request:
                    NetworkRequest::SendMessage {
                        dest,
                        payload,
                        responder,
                    },
            } => {
                self.send_message(*dest, *payload);
                responder.respond(()).ignore()
            }
            Event::NetworkRequest {
                request: NetworkRequest::Broadcast { payload, responder },
            } => {
                self.net_metrics.broadcast_requests.inc();
                self.gossip_message(*payload);
                responder.respond(()).ignore()
            }
            Event::NetworkRequest { request } => match request {
                NetworkRequest::SendMessage {
                    dest,
                    payload,
                    responder,
                } => {
                    self.net_metrics.direct_message_requests.inc();
                    self.send_message(*dest, *payload);
                    responder.respond(()).ignore()
                }
                NetworkRequest::Broadcast { payload, responder } => {
                    self.gossip_message(*payload);
                    responder.respond(()).ignore()
                }
                NetworkRequest::Gossip {
                    payload,
                    count,
                    exclude,
                    responder,
                } => {
                    let sent_to = self.send_message_to_n_peers(rng, *payload, count, exclude);
                    responder.respond(sent_to).ignore()
                }
            },
            Event::NetworkInfoRequest { info_request } => match info_request {
                NetworkInfoRequest::GetPeers { responder } => {
                    let peers = self
                        .peers
                        .iter()
                        .map(|(node_id, endpoint)| {
                            (node_id.clone(), endpoint.get_remote_address().to_string())
                        })
                        .collect();
                    responder.respond(peers).ignore()
                }
            },
        }
    }
}

/// An ephemeral [libp2p::identity::Keypair] which uniquely identifies this node
#[derive(Clone)]
pub struct NetworkIdentity {
    keypair: Keypair,
}

impl Debug for NetworkIdentity {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "NetworkIdentity(public key: {:?})",
            self.keypair.public()
        )
    }
}

impl NetworkIdentity {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let keypair = Keypair::generate_ed25519();
        NetworkIdentity { keypair }
    }
}

impl<REv, P> From<&Network<REv, P>> for NetworkIdentity {
    fn from(network: &Network<REv, P>) -> Self {
        network.network_identity.clone()
    }
}

impl From<&NetworkIdentity> for PeerId {
    fn from(network_identity: &NetworkIdentity) -> Self {
        PeerId::from(network_identity.keypair.public())
    }
}

impl From<&NetworkIdentity> for NodeId {
    fn from(network_identity: &NetworkIdentity) -> Self {
        NodeId::from(PeerId::from(network_identity))
    }
}
