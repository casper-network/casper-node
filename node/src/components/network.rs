mod config;
mod error;
mod event;
mod message;

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    env,
    fmt::{self, Debug, Display, Formatter},
    io,
    marker::PhantomData,
    num::NonZeroU32,
    time::Duration,
};

use datasize::DataSize;
use futures::{
    future::{self, BoxFuture, Either},
    FutureExt,
};
use libp2p::{
    core::{
        connection::{ConnectedPoint, PendingConnectionError},
        upgrade,
    },
    identity::Keypair,
    noise::{self, NoiseConfig, X25519Spec},
    ping::{Ping, PingConfig, PingEvent},
    swarm::{NetworkBehaviourEventProcess, SwarmBuilder, SwarmEvent},
    tcp::TokioTcpConfig,
    yamux::Config as YamuxConfig,
    Multiaddr, NetworkBehaviour, PeerId, Swarm, Transport,
};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{sync::watch, task::JoinHandle};
use tracing::{debug, error, info, trace, warn};

use crate::{
    components::Component,
    effect::{
        announcements::NetworkAnnouncement,
        requests::{NetworkInfoRequest, NetworkRequest},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    reactor::{EventQueueHandle, Finalize, QueueKind},
    types::NodeId,
    utils::DisplayIter,
    NodeRng,
};
pub use config::Config;
pub use error::Error;
pub(crate) use event::Event;
pub(crate) use message::Message;

/// The timeout for connection setup (including upgrades) for all inbound and outbound connections.
// TODO - make this a config option.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
/// Env var which, if it's defined at runtime, enables the libp2p server.
pub(crate) const ENABLE_LIBP2P_ENV_VAR: &str = "CASPER_ENABLE_LIBP2P";

/// A helper trait whose bounds represent the requirements for a payload that `Network` can
/// work with.
pub trait PayloadT:
    Serialize + DeserializeOwned + Clone + Debug + Display + Send + 'static
{
}

impl<P> PayloadT for P where
    P: Serialize + DeserializeOwned + Clone + Debug + Display + Send + 'static
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

#[derive(NetworkBehaviour)]
struct Behavior {
    #[behaviour(ignore)]
    our_id: NodeId,
    ping: Ping,
}

impl NetworkBehaviourEventProcess<PingEvent> for Behavior {
    fn inject_event(&mut self, event: PingEvent) {
        info!("{}: {:?}", self.our_id, event);
    }
}

#[derive(PartialEq, Eq, Debug, DataSize)]
enum ConnectionState {
    Pending,
    Connected,
    Failed,
}

#[derive(DataSize)]
pub(crate) struct Network<REv, P>
where
    REv: 'static,
{
    our_id: NodeId,
    peers: HashSet<NodeId>,
    #[data_size(skip)]
    listening_addresses: Vec<Multiaddr>,
    /// The addresses of known peers to be used for bootstrapping, and their connection states.
    #[data_size(skip)]
    known_addresses: HashMap<Multiaddr, ConnectionState>,
    /// Channel signaling a shutdown of the network component.
    #[data_size(skip)]
    shutdown_sender: Option<watch::Sender<()>>,
    server_join_handle: Option<JoinHandle<()>>,
    _phantom_data: PhantomData<(REv, P)>,
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
        notify: bool,
    ) -> Result<(Network<REv, P>, Effects<Event<P>>), Error> {
        // Create a new Ed25519 keypair for this session.
        let id_keys = Keypair::generate_ed25519();
        let our_peer_id = PeerId::from(id_keys.public());
        let our_id = NodeId::from(our_peer_id.clone());

        // Convert the known addresses to multiaddr format and prepare the shutdown signal.
        let known_addresses = config
            .known_addresses
            .iter()
            .map(|address| {
                let multiaddr = address_str_to_multiaddr(address.as_str());
                (multiaddr, ConnectionState::Pending)
            })
            .collect::<HashMap<_, _>>();
        let (server_shutdown_sender, server_shutdown_receiver) = watch::channel(());

        // If the env var "CASPER_ENABLE_LIBP2P" is not defined, exit without starting the server.
        if env::var(ENABLE_LIBP2P_ENV_VAR).is_err() {
            let network = Network {
                our_id,
                peers: HashSet::new(),
                listening_addresses: vec![],
                known_addresses,
                shutdown_sender: Some(server_shutdown_sender),
                server_join_handle: None,
                _phantom_data: PhantomData,
            };
            return Ok((network, Effects::new()));
        }

        if notify {
            debug!("our node id: {}", our_id);
        }

        // Create a keypair for authenticated encryption of the transport.
        let noise_keys = noise::Keypair::<X25519Spec>::new()
            .into_authentic(&id_keys)
            .map_err(Error::StaticKeypairSigning)?;

        // Create a tokio-based TCP transport.  Use `noise` for authenticated encryption and `yamux`
        // for multiplexing of substreams on a TCP stream.
        let transport = TokioTcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(YamuxConfig::default())
            .timeout(CONNECTION_TIMEOUT)
            .boxed();

        // Create a Swarm to manage peers and events.
        let mut swarm = {
            let ping = Ping::new(PingConfig::new().with_keep_alive(true));
            let behavior = Behavior {
                our_id: our_id.clone(),
                ping,
            };

            SwarmBuilder::new(transport, behavior, our_peer_id)
                .executor(Box::new(|future| {
                    tokio::spawn(future);
                }))
                .build()
        };

        // Schedule connection attempts to known peers.
        for address in known_addresses.keys() {
            Swarm::dial_addr(&mut swarm, address.clone()).map_err(|error| Error::DialPeer {
                address: address.clone(),
                error,
            })?;
        }

        // Specify listener.
        let listening_address = address_str_to_multiaddr(config.bind_address.as_str());
        Swarm::listen_on(&mut swarm, listening_address.clone()).map_err(|error| Error::Listen {
            address: listening_address,
            error,
        })?;

        // Start the server task.
        let server_join_handle = Some(tokio::spawn(server_task(
            event_queue,
            server_shutdown_receiver,
            our_id.clone(),
            swarm,
        )));

        let network = Network {
            our_id,
            peers: HashSet::new(),
            listening_addresses: vec![],
            known_addresses,
            shutdown_sender: Some(server_shutdown_sender),
            server_join_handle,
            _phantom_data: PhantomData,
        };
        Ok((network, Effects::new()))
    }

    fn handle_connection_established(
        &mut self,
        peer_id: NodeId,
        endpoint: ConnectedPoint,
        num_established: NonZeroU32,
    ) -> Effects<Event<P>> {
        debug!(%peer_id, ?endpoint, %num_established,"{}: connection established", self.our_id);
        let _ = self.peers.insert(peer_id);

        if let ConnectedPoint::Dialer { address } = endpoint {
            if let Some(state) = self.known_addresses.get_mut(&address) {
                if *state == ConnectionState::Pending {
                    *state = ConnectionState::Connected
                }
            }
        }

        Effects::new()
    }

    fn handle_unknown_peer_unreachable_address(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        address: Multiaddr,
        error: PendingConnectionError<io::Error>,
    ) -> Effects<Event<P>> {
        debug!(%address, %error, "{}: failed to connect", self.our_id);
        if let Some(state) = self.known_addresses.get_mut(&address) {
            if *state == ConnectionState::Pending {
                *state = ConnectionState::Failed
            }
        }

        // TODO - Consider how to handle becoming isolated _after_ we've bootstrapped.  Ensure we
        //        can't re-enter this block unless we implement re-bootstrapping for example.
        if self.is_isolated() {
            if self.is_bootstrap_node() {
                info!(
                    "{}: failed to bootstrap to any other nodes, but continuing to run as we are a \
                    bootstrap node", self.our_id
                );
            } else {
                // Note that we could retry the connection to other nodes, but for now we just
                // leave it up to the node operator to restart.
                return fatal!(
                    effect_builder,
                    "{}: failed to connect to any known node, now isolated",
                    self.our_id
                );
            }
        }
        Effects::new()
    }

    /// Returns whether or not this node has been isolated.
    ///
    /// An isolated node has no chance of recovering a connection to the network and is not
    /// connected to any peer.
    fn is_isolated(&self) -> bool {
        self.known_addresses
            .values()
            .all(|state| *state == ConnectionState::Failed)
    }

    /// Returns whether or not this node is listed as a bootstrap node.
    fn is_bootstrap_node(&self) -> bool {
        self.known_addresses
            .keys()
            .any(|address| self.listening_addresses.contains(address))
    }
}

async fn server_task<P, REv>(
    event_queue: EventQueueHandle<REv>,
    mut shutdown_receiver: watch::Receiver<()>,
    our_id: NodeId,
    mut swarm: Swarm<Behavior>,
) where
    REv: From<Event<P>>,
{
    let our_id_cloned = our_id.clone();
    let main_task = async move {
        loop {
            let swarm_event = swarm.next_event().await;
            trace!("{}: {:?}", our_id_cloned, swarm_event);
            let event = match swarm_event {
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    endpoint,
                    num_established,
                } => Event::ConnectionEstablished {
                    peer_id: NodeId::from(peer_id),
                    endpoint,
                    num_established,
                },
                SwarmEvent::ConnectionClosed {
                    peer_id,
                    endpoint,
                    num_established,
                    cause,
                } => Event::ConnectionClosed {
                    peer_id: NodeId::from(peer_id),
                    endpoint,
                    num_established,
                    cause: cause.map(|error| error.to_string()),
                },
                SwarmEvent::UnreachableAddr {
                    peer_id,
                    address,
                    error,
                    attempts_remaining,
                } => Event::UnreachableAddress {
                    peer_id: NodeId::from(peer_id),
                    address,
                    error,
                    attempts_remaining,
                },
                SwarmEvent::UnknownPeerUnreachableAddr { address, error } => {
                    Event::UnknownPeerUnreachableAddress { address, error }
                }
                SwarmEvent::NewListenAddr(address) => Event::NewListenAddress(address),
                SwarmEvent::ExpiredListenAddr(address) => Event::ExpiredListenAddress(address),
                SwarmEvent::ListenerClosed {
                    addresses,
                    reason: Ok(()),
                } => Event::ListenerClosed {
                    addresses,
                    reason: Ok(()),
                },
                SwarmEvent::ListenerClosed {
                    addresses,
                    reason: Err(error),
                } => Event::ListenerClosed {
                    addresses,
                    reason: Err(error),
                },
                SwarmEvent::ListenerError { error } => Event::ListenerError { error },
                SwarmEvent::Behaviour(_)
                | SwarmEvent::IncomingConnection { .. }
                | SwarmEvent::IncomingConnectionError { .. }
                | SwarmEvent::BannedPeer { .. }
                | SwarmEvent::Dialing(_) => continue,
            };
            event_queue.schedule(event, QueueKind::Network).await;
        }
    };

    let shutdown_messages = async move { while shutdown_receiver.recv().await.is_some() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match future::select(Box::pin(shutdown_messages), Box::pin(main_task)).await {
        Either::Left(_) => info!("{}: shutting down libp2p", our_id),
        Either::Right(_) => unreachable!(),
    }
}

/// Converts a string of the form "127.0.0.1:34553" into a Multiaddr equivalent to
/// "/ip4/127.0.0.1/tcp/34553".
fn address_str_to_multiaddr(address: &str) -> Multiaddr {
    let mut parts_itr = address.split(':');
    let multiaddr_str = format!(
        "/ip4/{}/tcp/{}",
        parts_itr.next().expect("address should contain IP segment"),
        parts_itr
            .next()
            .expect("address should contain port segment")
    );
    // OK to `expect` for now as this method will become redundant once small_network is removed.
    multiaddr_str
        .parse()
        .expect("address should parse as a multiaddr")
}

impl<REv, P> Finalize for Network<REv, P>
where
    REv: Send + 'static,
    P: Send + 'static,
{
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
            } else if env::var(ENABLE_LIBP2P_ENV_VAR).is_ok() {
                warn!("{}: server shutdown while already shut down", self.our_id)
            }
        }
        .boxed()
    }
}

impl<REv, P: Debug> Debug for Network<REv, P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Network")
            .field("our_id", &self.our_id)
            .field("peers", &self.peers)
            .field("listening_addresses", &self.listening_addresses)
            .field("known_addresses", &self.known_addresses)
            .finish()
    }
}

impl<REv: ReactorEventT<P>, P: PayloadT> Component<REv> for Network<REv, P> {
    type Event = Event<P>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!(?self, "{}: {:?}", self.our_id, event);
        match event {
            Event::ConnectionEstablished {
                peer_id,
                endpoint,
                num_established,
            } => self.handle_connection_established(peer_id, endpoint, num_established),
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
            Event::UnknownPeerUnreachableAddress { address, error } => {
                self.handle_unknown_peer_unreachable_address(effect_builder, address, error)
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

            Event::IncomingMessage { peer_id, msg: _ } => {
                trace!(%peer_id, "{}: incoming message", self.our_id);
                // self.handle_message(effect_builder, peer_id, msg)
                Effects::new()
            }
            Event::NetworkRequest {
                request:
                    NetworkRequest::SendMessage {
                        dest,
                        payload: _,
                        responder,
                    },
            } => {
                // We're given a message to send out.
                trace!(%dest, "{}: request to send message", self.our_id);
                // self.send_message(dest, Message(payload));
                responder.respond(()).ignore()
            }
            Event::NetworkRequest {
                request:
                    NetworkRequest::Broadcast {
                        payload: _,
                        responder,
                    },
            } => {
                // We're given a message to broadcast.
                trace!("{}: request to broadcast message", self.our_id);
                // self.broadcast_message(Message(payload));
                responder.respond(()).ignore()
            }
            Event::NetworkRequest {
                request:
                    NetworkRequest::Gossip {
                        payload: _,
                        count: _,
                        exclude: _,
                        responder,
                    },
            } => {
                // We're given a message to gossip.
                trace!("{}: request to broadcast message", self.our_id);
                // let sent_to = self.gossip_message(rng, Message(payload), count, exclude);
                responder.respond(HashSet::new()).ignore()
            }
            Event::NetworkInfoRequest {
                info_request: NetworkInfoRequest::GetPeers { responder },
            } => {
                trace!("{}: request for info", self.our_id);
                responder.respond(HashMap::new()).ignore()
            }
        }
    }
}
