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
//! connected to the correct node and sends its own certificate during the TLS handshake,
//! establishing identity.
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
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    convert::Infallible,
    env,
    fmt::{self, Debug, Display, Formatter},
    io,
    net::{SocketAddr, TcpListener},
    pin::Pin,
    result,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use datasize::DataSize;
use futures::{
    future::{self, BoxFuture, Either},
    stream::{SplitSink, SplitStream},
    FutureExt, SinkExt, StreamExt,
};
use once_cell::sync::Lazy;
use openssl::{error::ErrorStack as OpenSslErrorStack, pkey, ssl::Ssl};
use pkey::{PKey, Private};
use prometheus::{IntGauge, Registry};
use rand::seq::IteratorRandom;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        watch,
    },
    task::JoinHandle,
};
use tokio_openssl::SslStream;
use tokio_serde::SymmetricallyFramed;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{debug, error, info, trace, warn};

use self::{
    counting_format::{ConnectionId, CountingFormat, Role},
    error::Result,
    message_pack_format::MessagePackFormat,
};
pub(crate) use self::{
    error::display_error, event::Event, gossiped_address::GossipedAddress, message::Message,
};
use crate::{
    components::{
        network::ENABLE_LIBP2P_NET_ENV_VAR, networking_metrics::NetworkingMetrics, Component,
    },
    effect::{
        announcements::{BlocklistAnnouncement, NetworkAnnouncement},
        requests::{NetworkInfoRequest, NetworkRequest},
        EffectBuilder, EffectExt, EffectResultExt, Effects,
    },
    reactor::{EventQueueHandle, Finalize, QueueKind, ReactorEvent},
    tls::{self, TlsCert, ValidationError},
    types::{NodeId, TimeDiff, Timestamp},
    utils, NodeRng,
};
use chain_info::ChainInfo;
pub use config::Config;
pub use error::Error;

const MAX_ASYMMETRIC_CONNECTION_SEEN: u16 = 4;
static BLOCKLIST_RETAIN_DURATION: Lazy<TimeDiff> =
    Lazy::new(|| Duration::from_secs(60 * 10).into());

/// Network message payload.
///
/// Payloads are what is transferred across the network outside of control messages from the
/// networking component itself.
pub trait Payload: Serialize + DeserializeOwned + Clone + Debug + Display + Send + 'static {
    /// Classifies the payload based on its contents.
    fn classify(&self) -> MessageKind;
}

/// A classification system for networking messages.
#[derive(Copy, Clone, Debug)]
pub enum MessageKind {
    /// Non-payload messages, like handshakes.
    Protocol,
    /// Messages directly related to consensus.
    Consensus,
    /// Deploys being gossiped.
    DeployGossip,
    /// Addresses begin gossiped.
    AddressGossip,
    /// Deploys being transferred directly (via requests).
    DeployTransfer,
    /// Blocks for finality signatures being transferred directly (via requests and other means).
    BlockTransfer,
    /// Any other kind of payload (or missing classification).
    Other,
}

impl Display for MessageKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MessageKind::Protocol => f.write_str("protocol"),
            MessageKind::Consensus => f.write_str("consensus"),
            MessageKind::DeployGossip => f.write_str("deploy_gossip"),
            MessageKind::AddressGossip => f.write_str("address_gossip"),
            MessageKind::DeployTransfer => f.write_str("deploy_transfer"),
            MessageKind::BlockTransfer => f.write_str("block_transfer"),
            MessageKind::Other => f.write_str("other"),
        }
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct OutgoingConnection<P> {
    #[data_size(skip)] // Unfortunately, there is no way to inspect an `UnboundedSender`.
    sender: UnboundedSender<Message<P>>,
    peer_address: SocketAddr,

    // for keeping track of connection asymmetry, tracking the number of times we've seen this
    // connection be asymmetric.
    times_seen_asymmetric: u16,
}

#[derive(DataSize, Debug)]
pub(crate) struct IncomingConnection {
    peer_address: SocketAddr,

    // for keeping track of connection asymmetry, tracking the number of times we've seen this
    // connection be asymmetric.
    times_seen_asymmetric: u16,
}

#[derive(DataSize)]
pub(crate) struct SmallNetwork<REv, P>
where
    REv: 'static,
{
    /// Initial configuration values.
    cfg: Config,
    /// Server certificate.
    certificate: Arc<TlsCert>,
    /// Server secret key.
    secret_key: Arc<PKey<Private>>,
    /// Our public listening address.
    public_address: SocketAddr,
    /// Our node ID,
    our_id: NodeId,
    /// If we connect to ourself, this flag is set to true.
    is_bootstrap_node: bool,
    /// Handle to event queue.
    event_queue: EventQueueHandle<REv>,

    /// Incoming network connection addresses.
    incoming: HashMap<NodeId, IncomingConnection>,
    /// Outgoing network connections' messages.
    outgoing: HashMap<NodeId, OutgoingConnection<P>>,

    /// List of addresses which this node will avoid connecting to and the time they were added.
    blocklist: HashMap<SocketAddr, Timestamp>,

    /// Pending outgoing connections: ones for which we are currently trying to make a connection.
    pending: HashMap<SocketAddr, Instant>,

    /// Information retained from the chainspec required for operating the networking component.
    chain_info: Arc<ChainInfo>,

    /// Channel signaling a shutdown of the small network.
    // Note: This channel is closed when `SmallNetwork` is dropped, signalling the receivers that
    // they should cease operation.
    #[data_size(skip)]
    shutdown_sender: Option<watch::Sender<()>>,
    /// A clone of the receiver is passed to the message reader for all new incoming connections in
    /// order that they can be gracefully terminated.
    #[data_size(skip)]
    shutdown_receiver: watch::Receiver<()>,
    /// Flag to indicate the server has stopped running.
    is_stopped: Arc<AtomicBool>,
    /// Join handle for the server thread.
    #[data_size(skip)]
    server_join_handle: Option<JoinHandle<()>>,

    /// Networking metrics.
    #[data_size(skip)]
    net_metrics: Arc<NetworkingMetrics>,

    /// Known addresses for this node.
    known_addresses: HashSet<SocketAddr>,
}

impl<REv, P> SmallNetwork<REv, P>
where
    P: Payload + 'static,
    REv: ReactorEvent + From<Event<P>> + From<NetworkAnnouncement<NodeId, P>>,
{
    /// Creates a new small network component instance.
    ///
    /// If `notify` is set to `false`, no systemd notifications will be sent, regardless of
    /// configuration.
    #[allow(clippy::type_complexity)]
    pub(crate) fn new<C: Into<ChainInfo>>(
        event_queue: EventQueueHandle<REv>,
        cfg: Config,
        registry: &Registry,
        small_network_identity: SmallNetworkIdentity,
        chain_info_source: C,
        notify: bool,
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

        let mut public_address =
            utils::resolve_address(&cfg.public_address).map_err(Error::ResolveAddr)?;

        let our_id = NodeId::from(&small_network_identity);
        let secret_key = small_network_identity.secret_key;
        let certificate = small_network_identity.tls_certificate;

        let chain_info = Arc::new(chain_info_source.into());

        // If the env var "CASPER_ENABLE_LIBP2P_NET" is defined, exit without starting the
        // server.
        if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_ok() {
            let model = SmallNetwork {
                cfg,
                known_addresses,
                certificate,
                secret_key,
                public_address,
                our_id,
                is_bootstrap_node: false,
                event_queue,
                incoming: HashMap::new(),
                outgoing: HashMap::new(),
                pending: HashMap::new(),
                blocklist: HashMap::new(),
                chain_info,
                shutdown_sender: None,
                shutdown_receiver: watch::channel(()).1,
                server_join_handle: None,
                is_stopped: Arc::new(AtomicBool::new(true)),
                net_metrics: Arc::new(NetworkingMetrics::new(&Registry::default())?),
            };
            return Ok((model, Effects::new()));
        }

        let net_metrics = NetworkingMetrics::new(&registry)?;

        // We can now create a listener.
        let bind_address = utils::resolve_address(&cfg.bind_address).map_err(Error::ResolveAddr)?;
        let listener = TcpListener::bind(bind_address)
            .map_err(|error| Error::ListenerCreation(error, bind_address))?;
        // We must set non-blocking to `true` or else the tokio task hangs forever.
        listener
            .set_nonblocking(true)
            .map_err(Error::ListenerSetNonBlocking)?;

        // Once the port has been bound, we can notify systemd if instructed to do so.
        if notify {
            if cfg.systemd_support {
                if sd_notify::booted().map_err(Error::SystemD)? {
                    info!("notifying systemd that the network is ready to receive connections");
                    sd_notify::notify(true, &[sd_notify::NotifyState::Ready])
                        .map_err(Error::SystemD)?;
                } else {
                    warn!("systemd_support enabled but not booted with systemd, ignoring");
                }
            } else {
                debug!("systemd_support disabled, not notifying");
            }
        }
        let local_address = listener.local_addr().map_err(Error::ListenerAddr)?;

        // Substitute the actually bound port if set to 0.
        if public_address.port() == 0 {
            public_address.set_port(local_address.port());
        }

        // Run the server task.
        // We spawn it ourselves instead of through an effect to get a hold of the join handle,
        // which we need to shutdown cleanly later on.
        info!(%local_address, %public_address, "{}: starting server background task", our_id);
        let (server_shutdown_sender, server_shutdown_receiver) = watch::channel(());
        let shutdown_receiver = server_shutdown_receiver.clone();
        let server_join_handle = tokio::spawn(server_task(
            event_queue,
            tokio::net::TcpListener::from_std(listener).map_err(Error::ListenerConversion)?,
            server_shutdown_receiver,
            our_id,
        ));

        let mut model = SmallNetwork {
            cfg,
            known_addresses,
            certificate,
            secret_key,
            public_address,
            our_id,
            is_bootstrap_node: false,
            event_queue,
            incoming: HashMap::new(),
            outgoing: HashMap::new(),
            pending: HashMap::new(),
            blocklist: HashMap::new(),
            chain_info,
            shutdown_sender: Some(server_shutdown_sender),
            shutdown_receiver,
            server_join_handle: Some(server_join_handle),
            is_stopped: Arc::new(AtomicBool::new(false)),
            net_metrics: Arc::new(net_metrics),
        };

        // Bootstrap process.
        let effect_builder = EffectBuilder::new(event_queue);

        // We kick things off by adding effects to connect to all known addresses. This will
        // automatically attempt to repeat the connection process if it fails (see
        // `connect_to_known_addresses` for details).
        let mut effects = model.connect_to_known_addresses();

        // Start broadcasting our public listening address.
        effects.extend(
            effect_builder
                .set_timeout(model.cfg.initial_gossip_delay.into())
                .event(|_| Event::GossipOurAddress),
        );

        Ok((model, effects))
    }

    /// Queues a message to be sent to all nodes.
    fn broadcast_message(&self, msg: Message<P>) {
        for peer_id in self.outgoing.keys() {
            self.send_message(*peer_id, msg.clone());
        }
    }

    /// Try to establish a connection to all known addresses in the configuration.
    ///
    /// Will schedule another reconnection if no DNS addresses could be resolved.
    fn connect_to_known_addresses(&mut self) -> Effects<Event<P>> {
        let mut effects = Effects::new();

        let now = Instant::now();
        for &address in &self.known_addresses {
            self.pending.insert(address, now);

            // Add an effect to connect to the known address.
            effects.extend(
                connect_outgoing(
                    address,
                    Arc::clone(&self.certificate),
                    Arc::clone(&self.secret_key),
                    Arc::clone(&self.is_stopped),
                )
                .result(
                    move |(peer_id, transport)| Event::OutgoingEstablished {
                        peer_id: Box::new(peer_id),
                        transport,
                    },
                    move |error| Event::OutgoingFailed {
                        peer_address: Box::new(address),
                        peer_id: Box::new(None),
                        error: Box::new(Some(error)),
                    },
                ),
            );
        }

        effects
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
            .outgoing
            .keys()
            .filter(|&peer_id| !exclude.contains(peer_id))
            .choose_multiple(rng, count);

        if peer_ids.len() != count {
            // TODO - set this to `warn!` once we are normally testing with networks large enough to
            //        make it a meaningful and infrequent log message.
            trace!(
                our_id=%self.our_id,
                wanted = count,
                selected = peer_ids.len(),
                "could not select enough random nodes for gossiping, not enough non-excluded \
                outgoing connections"
            );
        }

        for &&peer_id in &peer_ids {
            self.send_message(peer_id, msg.clone());
        }

        peer_ids.into_iter().copied().collect()
    }

    /// Queues a message to be sent to a specific node.
    fn send_message(&self, dest: NodeId, msg: Message<P>) {
        // Try to send the message.
        if let Some(connection) = self.outgoing.get(&dest) {
            if let Err(msg) = connection.sender.send(msg) {
                // We lost the connection, but that fact has not reached us yet.
                warn!(our_id=%self.our_id, %dest, ?msg, "dropped outgoing message, lost connection");
            } else {
                self.net_metrics.queued_messages.inc();
            }
        } else {
            // We are not connected, so the reconnection is likely already in progress.
            debug!(our_id=%self.our_id, %dest, ?msg, "dropped outgoing message, no connection");
        }
    }

    /// Sweep and timeout pending connections.
    ///
    /// This is a reliability measure that sweeps pending connections, since leftover entries will
    /// block any renewed connection attempts to a specific address.
    ///
    /// In 100% bug free code, this would not be necessary, so this is in here as a stop-gap measure
    /// to avoid locking up a node with unremoved pending connections.
    ///
    /// Connections will only be removed from pending, they will NOT be forcefully disconnected.
    fn sweep_pending_connections(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<P>> {
        let now = Instant::now();
        let max_addr_pending_time: Duration = self.cfg.max_addr_pending_time.into();

        // Remove pending connections that have been pending for a long time. Ideally, we would use
        // `drain_filter` here, but it is still unstable, so just collect the keys.
        let outdated_keys: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(&addr, &timestamp)| {
                if now - timestamp > max_addr_pending_time {
                    Some(addr)
                } else {
                    None
                }
            })
            .collect();

        outdated_keys.iter().for_each(|key| {
            warn!(addr=%key, "swept pending address");
            self.pending.remove(key);
        });

        effect_builder
            .set_timeout(max_addr_pending_time / 2)
            .event(|_| Event::SweepPending)
    }

    fn handle_incoming_tls_handshake_completed(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        result: Result<(NodeId, Transport)>,
        peer_address: SocketAddr,
    ) -> Effects<Event<P>> {
        match result {
            Ok((peer_id, transport)) => {
                // If we have connected to ourself, allow the connection to drop.
                if peer_id == self.our_id {
                    self.is_bootstrap_node = true;
                    debug!(
                        our_id=%self.our_id,
                        %peer_address,
                        local_address=?transport.get_ref().local_addr(),
                        "connected incoming to ourself - closing connection"
                    );
                    return Effects::new();
                }

                // If the peer has already disconnected, allow the connection to drop.
                if let Err(ref err) = transport.get_ref().peer_addr() {
                    debug!(
                        our_id=%self.our_id,
                        %peer_address,
                        local_address=?transport.get_ref().local_addr(),
                        err=display_error(err),
                        "incoming connection dropped",
                    );
                    return Effects::new();
                }

                info!(our_id=%self.our_id, %peer_id, %peer_address, "established incoming connection");
                // The sink is only used to send a single handshake message, then dropped.
                let (mut sink, stream) = framed::<P>(
                    self.net_metrics.clone(),
                    ConnectionId::from_connection(transport.ssl(), self.our_id, peer_id),
                    transport,
                    Role::Listener,
                    self.chain_info.maximum_net_message_size,
                )
                .split();
                let handshake = self.chain_info.create_handshake(self.public_address);
                let mut effects = async move {
                    let _ = sink.send(handshake).await;
                }
                .ignore::<Event<P>>();

                let _ = self.incoming.insert(
                    peer_id,
                    IncomingConnection {
                        peer_address,
                        times_seen_asymmetric: 0,
                    },
                );
                self.net_metrics
                    .open_connections
                    .set(self.incoming.len() as i64);

                // If the connection is now complete, announce the new peer before starting reader.
                effects.extend(self.check_connection_complete(effect_builder, peer_id));

                effects.extend(
                    message_reader(
                        self.event_queue,
                        stream,
                        self.shutdown_receiver.clone(),
                        self.our_id,
                        peer_id,
                    )
                    .event(move |result| Event::IncomingClosed {
                        result,
                        peer_id: Box::new(peer_id),
                        peer_address: Box::new(peer_address),
                    }),
                );

                effects
            }
            Err(ref err) => {
                warn!(our_id=%self.our_id, %peer_address, err=display_error(err), "TLS handshake failed");
                Effects::new()
            }
        }
    }

    /// Sets up an established outgoing connection.
    fn setup_outgoing(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
        transport: Transport,
    ) -> Effects<Event<P>> {
        // This connection is send-only, we only use the sink.
        let peer_address = match transport.get_ref().peer_addr() {
            Ok(peer_addr) => peer_addr,
            Err(ref err) => {
                // The peer address disappeared, likely because the connection was closed while
                // we are setting up.
                warn!(%peer_id, err=display_error(err), "peer connection terminated while setting up outgoing connection, dropping");

                // We still need to clean up any trace of the connection.
                return self.remove(effect_builder, &peer_id, false);
            }
        };

        // Remove from pending connection set, but ignore if it is missing.
        self.pending.remove(&peer_address);

        // If we have connected to ourself, allow the connection to drop.
        if peer_id == self.our_id {
            self.is_bootstrap_node = true;
            debug!(
                our_id=%self.our_id,
                peer_address=?transport.get_ref().peer_addr(),
                local_address=?transport.get_ref().local_addr(),
                "connected outgoing to ourself - closing connection",
            );
            return self.reconnect_if_not_connected_to_any_known_addresses(effect_builder);
        }

        // The stream is only used to receive a single handshake message and then dropped.
        let (sink, stream) = framed::<P>(
            self.net_metrics.clone(),
            ConnectionId::from_connection(transport.ssl(), self.our_id, peer_id),
            transport,
            Role::Dialer,
            self.chain_info.maximum_net_message_size,
        )
        .split();
        debug!(our_id=%self.our_id, %peer_id, %peer_address, "established outgoing connection");

        let (sender, receiver) = mpsc::unbounded_channel();
        let connection = OutgoingConnection {
            peer_address,
            sender,
            times_seen_asymmetric: 0,
        };
        if self.outgoing.insert(peer_id, connection).is_some() {
            // We assume that for a reconnect to have happened, the outgoing entry must have
            // been either non-existent yet or cleaned up by the handler of the connection
            // closing event. If this is not the case, an assumed invariant has been violated.
            error!(our_id=%self.our_id, %peer_id, "did not expect leftover channel in outgoing map");
        }

        let mut effects = self.check_connection_complete(effect_builder, peer_id);

        let handshake = self.chain_info.create_handshake(self.public_address);

        effects.extend(
            message_sender(
                receiver,
                sink,
                self.net_metrics.queued_messages.clone(),
                handshake,
            )
            .event(move |result| Event::OutgoingFailed {
                peer_id: Box::new(Some(peer_id)),
                peer_address: Box::new(peer_address),
                error: Box::new(result.err().map(Into::into)),
            }),
        );
        effects.extend(
            handshake_reader(self.event_queue, stream, self.our_id, peer_id, peer_address)
                .ignore::<Event<P>>(),
        );

        effects
    }

    fn handle_outgoing_lost(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: Option<NodeId>,
        peer_address: SocketAddr,
        error: Option<Error>,
    ) -> Effects<Event<P>> {
        let _ = self.pending.remove(&peer_address);

        if let Some(peer_id) = peer_id {
            if let Some(ref err) = error {
                warn!(
                    our_id=%self.our_id,
                    %peer_id,
                    %peer_address,
                    err=display_error(err),
                    "outgoing connection failed"
                );
            } else {
                warn!(our_id=%self.our_id, %peer_id, %peer_address, "outgoing connection closed");
            }
            return self.remove(effect_builder, &peer_id, false);
        }

        // If we don't have the node ID passed in here, it was never added as an
        // outgoing connection, hence no need to call `self.remove()`.
        if let Some(ref err) = error {
            warn!(
                our_id=%self.our_id,
                %peer_address,
                err=display_error(err),
                "outgoing connection to known address failed"
            );
        } else {
            warn!(
                our_id=%self.our_id,
                %peer_address,
                "outgoing connection to known address closed"
            );
        }
        // Since we are not calling `self.remove()`, call the reconnection check explicitly.
        self.reconnect_if_not_connected_to_any_known_addresses(effect_builder)
    }

    fn remove(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        peer_id: &NodeId,
        add_to_blocklist: bool,
    ) -> Effects<Event<P>> {
        if let Some(incoming) = self.incoming.remove(&peer_id) {
            trace!(our_id=%self.our_id, %peer_id, "removing peer from the incoming connections");
            let _ = self.pending.remove(&incoming.peer_address);

            self.net_metrics
                .open_connections
                .set(self.incoming.len() as i64);
        }
        if let Some(outgoing) = self.outgoing.remove(&peer_id) {
            trace!(our_id=%self.our_id, %peer_id, "removing peer from the outgoing connections");
            if add_to_blocklist && !self.known_addresses.contains(&outgoing.peer_address) {
                info!(our_id=%self.our_id, %peer_id, "blocklisting peer");
                self.blocklist
                    .insert(outgoing.peer_address, Timestamp::now());
            }
        }

        self.reconnect_if_not_connected_to_any_known_addresses(effect_builder)
    }

    /// Gossips our public listening address, and schedules the next such gossip round.
    fn gossip_our_address(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event<P>> {
        let our_address = GossipedAddress::new(self.public_address);
        let mut effects = effect_builder
            .announce_gossip_our_address(our_address)
            .ignore();
        effects.extend(
            effect_builder
                .set_timeout(self.cfg.gossip_interval)
                .event(|_| Event::GossipOurAddress),
        );
        effects
    }

    /// Marks connections as asymmetric (only incoming or only outgoing) and removes them if they
    /// pass the upper limit for this. Connections that are symmetrical are reset to 0.
    fn enforce_symmetric_connections(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<P>> {
        let mut remove = Vec::new();
        for (node_id, conn) in self.incoming.iter_mut() {
            if !self.outgoing.contains_key(node_id) {
                if conn.times_seen_asymmetric >= MAX_ASYMMETRIC_CONNECTION_SEEN {
                    remove.push(*node_id);
                } else {
                    conn.times_seen_asymmetric += 1;
                }
            } else {
                conn.times_seen_asymmetric = 0;
            }
        }
        for (node_id, conn) in self.outgoing.iter_mut() {
            if !self.incoming.contains_key(node_id) {
                if conn.times_seen_asymmetric >= MAX_ASYMMETRIC_CONNECTION_SEEN {
                    remove.push(*node_id);
                } else {
                    conn.times_seen_asymmetric += 1;
                }
            } else {
                conn.times_seen_asymmetric = 0;
            }
        }
        let mut effects = Effects::new();
        for node_id in remove {
            effects.extend(self.remove(effect_builder, &node_id, true));
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
                public_address,
                protocol_version,
            } => {
                if network_name != self.chain_info.network_name {
                    info!(
                        our_id=%self.our_id,
                        %peer_id,
                        our_network=?self.chain_info.network_name,
                        their_network=?network_name,
                        our_protocol_version=%self.chain_info.protocol_version,
                        their_protocol_version=%protocol_version,
                        "dropping connection due to network name mismatch"
                    );
                    let remove = self.remove(effect_builder, &peer_id, false);
                    self.update_peers_metric();
                    return remove;
                }

                // This speeds up the connection process, but masks potential bugs in the gossiper.
                let effects = self.connect_to_peer_if_required(public_address);
                self.update_peers_metric();

                effects
            }
            Message::Payload(payload) => effect_builder
                .announce_message_received(peer_id, payload)
                .ignore(),
        }
    }

    fn update_peers_metric(&mut self) {
        self.net_metrics.peers.set(self.peers().len() as i64);
    }

    fn connect_to_peer_if_required(&mut self, peer_address: SocketAddr) -> Effects<Event<P>> {
        let now = Timestamp::now();
        self.blocklist
            .retain(|_, ts| *ts > now - *BLOCKLIST_RETAIN_DURATION);
        if self.pending.contains_key(&peer_address)
            || self.blocklist.contains_key(&peer_address)
            || self
                .outgoing
                .iter()
                .any(|(_peer_id, connection)| connection.peer_address == peer_address)
        {
            // We're already trying to connect, are connected, or the connection is on the blocklist
            // - do nothing.
            Effects::new()
        } else {
            // We need to connect.
            let now = Instant::now();
            self.pending.insert(peer_address, now);
            connect_outgoing(
                peer_address,
                Arc::clone(&self.certificate),
                Arc::clone(&self.secret_key),
                Arc::clone(&self.is_stopped),
            )
            .result(
                move |(peer_id, transport)| Event::OutgoingEstablished {
                    peer_id: Box::new(peer_id),
                    transport,
                },
                move |error| Event::OutgoingFailed {
                    peer_id: Box::new(None),
                    peer_address: Box::new(peer_address),
                    error: Box::new(Some(error)),
                },
            )
        }
    }

    /// Checks whether a connection has been established fully, i.e. with an incoming and outgoing
    /// connection.
    ///
    /// Returns either no effect or an announcement that a new peer has connected.
    fn check_connection_complete(
        &self,
        effect_builder: EffectBuilder<REv>,
        peer_id: NodeId,
    ) -> Effects<Event<P>> {
        if self.outgoing.contains_key(&peer_id) && self.incoming.contains_key(&peer_id) {
            debug!(%peer_id, "connection to peer is now complete");
            effect_builder.announce_new_peer(peer_id).ignore()
        } else {
            Effects::new()
        }
    }

    /// If we are isolated, try to reconnect to all known nodes.
    fn reconnect_if_not_connected_to_any_known_addresses(
        &self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event<P>> {
        if self.is_not_connected_to_any_known_address() {
            info!(delay=?self.cfg.isolation_reconnect_delay, "we are isolated. will attempt to reconnect to all known nodes after a delay");

            effect_builder
                .set_timeout(self.cfg.isolation_reconnect_delay.into())
                .event(|_| Event::IsolationReconnection)
        } else {
            Effects::new()
        }
    }

    /// Returns the set of connected nodes.
    pub(crate) fn peers(&self) -> BTreeMap<NodeId, String> {
        let mut ret = BTreeMap::new();
        for (node_id, connection) in &self.outgoing {
            ret.insert(*node_id, connection.peer_address.to_string());
        }
        for (node_id, connection) in &self.incoming {
            ret.entry(*node_id)
                .or_insert_with(|| connection.peer_address.to_string());
        }
        ret
    }

    /// Returns whether or not this node has been disconnected from all known nodes.
    fn is_not_connected_to_any_known_address(&self) -> bool {
        for &known_address in &self.known_addresses {
            if self.pending.contains_key(&known_address) {
                return false;
            }

            if self
                .outgoing
                .values()
                .any(|outgoing_connection| outgoing_connection.peer_address == known_address)
            {
                return false;
            }
        }

        true
    }

    /// Returns the node id of this network node.
    /// - Used in validator test.
    #[cfg(test)]
    pub(crate) fn node_id(&self) -> NodeId {
        self.our_id
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
            drop(self.shutdown_sender.take());

            // Set the flag to true, ensuring any ongoing attempts to establish outgoing TLS
            // connections return errors.
            self.is_stopped.store(true, Ordering::SeqCst);

            // Wait for the server to exit cleanly.
            if let Some(join_handle) = self.server_join_handle.take() {
                match join_handle.await {
                    Ok(_) => debug!(our_id=%self.our_id, "server exited cleanly"),
                    Err(ref err) => {
                        error!(%self.our_id, err=display_error(err), "could not join server task cleanly")
                    }
                }
            } else if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_err() {
                warn!(our_id=%self.our_id, "server shutdown while already shut down")
            }
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
            Event::IsolationReconnection => {
                if self.is_not_connected_to_any_known_address() {
                    info!("still isolated after grace time, attempting to reconnect to all known_nodes");
                    self.connect_to_known_addresses()
                } else {
                    info!("would attempt to reconnect, but no longer isolated. not reconnecting");
                    Effects::new()
                }
            }
            Event::IncomingNew {
                stream,
                peer_address,
            } => {
                debug!(our_id=%self.our_id, %peer_address, "incoming connection, starting TLS handshake");

                setup_tls(stream, self.certificate.clone(), self.secret_key.clone())
                    .boxed()
                    .event(move |result| Event::IncomingHandshakeCompleted {
                        result: Box::new(result),
                        peer_address,
                    })
            }
            Event::IncomingHandshakeCompleted {
                result,
                peer_address,
            } => {
                self.handle_incoming_tls_handshake_completed(effect_builder, *result, *peer_address)
            }
            Event::IncomingMessage { peer_id, msg } => {
                self.handle_message(effect_builder, *peer_id, *msg)
            }
            Event::IncomingClosed {
                result,
                peer_id,
                peer_address,
            } => {
                match result {
                    Ok(()) => {
                        info!(our_id=%self.our_id, %peer_id, %peer_address, "connection closed",)
                    }
                    Err(ref err) => {
                        warn!(our_id=%self.our_id, %peer_id, %peer_address, err=display_error(err), "connection dropped")
                    }
                }
                self.remove(effect_builder, &peer_id, false)
            }
            Event::OutgoingEstablished { peer_id, transport } => {
                self.setup_outgoing(effect_builder, *peer_id, transport)
            }
            Event::OutgoingFailed {
                peer_id,
                peer_address,
                error,
            } => self.handle_outgoing_lost(effect_builder, *peer_id, *peer_address, *error),
            Event::SweepPending => self.sweep_pending_connections(effect_builder),
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
            Event::GossipOurAddress => {
                let mut effects = self.gossip_our_address(effect_builder);
                effects.extend(self.enforce_symmetric_connections(effect_builder));
                effects
            }
            Event::PeerAddressReceived(gossiped_address) => {
                self.connect_to_peer_if_required(gossiped_address.into())
            }
            Event::BlocklistAnnouncement(BlocklistAnnouncement::OffenseCommitted(ref peer_id)) => {
                warn!(%peer_id, "adding peer to blocklist after transgression");
                self.remove(effect_builder, peer_id, true)
            }
        }
    }
}

/// Core accept loop for the networking server.
///
/// Never terminates.
async fn server_task<P, REv>(
    event_queue: EventQueueHandle<REv>,
    listener: tokio::net::TcpListener,
    mut shutdown_receiver: watch::Receiver<()>,
    our_id: NodeId,
) where
    REv: From<Event<P>>,
{
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
                Ok((stream, peer_address)) => {
                    // Move the incoming connection to the event queue for handling.
                    let event = Event::IncomingNew {
                        stream,
                        peer_address: Box::new(peer_address),
                    };
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
                Err(ref err) => {
                    warn!(%our_id, err=display_error(err), "dropping incoming connection during accept")
                }
            }
        }
    };

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match future::select(Box::pin(shutdown_messages), Box::pin(accept_connections)).await {
        Either::Left(_) => info!(
            %our_id,
            "shutting down socket, no longer accepting incoming connections"
        ),
        Either::Right(_) => unreachable!(),
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

impl<REv, P> From<&SmallNetwork<REv, P>> for SmallNetworkIdentity {
    fn from(small_network: &SmallNetwork<REv, P>) -> Self {
        SmallNetworkIdentity {
            secret_key: small_network.secret_key.clone(),
            tls_certificate: small_network.certificate.clone(),
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

/// Server-side TLS handshake.
///
/// This function groups the TLS handshake into a convenient function, enabling the `?` operator.
async fn setup_tls(
    stream: TcpStream,
    cert: Arc<TlsCert>,
    secret_key: Arc<PKey<Private>>,
) -> Result<(NodeId, Transport)> {
    let mut tls_stream = tls::create_tls_acceptor(&cert.as_x509().as_ref(), &secret_key.as_ref())
        .and_then(|ssl_acceptor| Ssl::new(ssl_acceptor.context()))
        .and_then(|ssl| SslStream::new(ssl, stream))
        .map_err(Error::AcceptorCreation)?;

    SslStream::accept(Pin::new(&mut tls_stream))
        .await
        .map_err(Error::Handshake)?;

    // We can now verify the certificate.
    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or(Error::NoClientCertificate)?;

    Ok((
        NodeId::from(tls::validate_cert(peer_cert)?.public_key_fingerprint()),
        tls_stream,
    ))
}

/// Network handshake reader for single handshake message received by outgoing connection.
async fn handshake_reader<REv, P>(
    event_queue: EventQueueHandle<REv>,
    mut stream: SplitStream<FramedTransport<P>>,
    our_id: NodeId,
    peer_id: NodeId,
    peer_address: SocketAddr,
) where
    P: DeserializeOwned + Send + Display + Payload,
    REv: From<Event<P>>,
{
    if let Some(Ok(msg @ Message::Handshake { .. })) = stream.next().await {
        debug!(%our_id, %msg, %peer_id, "handshake received");
        return event_queue
            .schedule(
                Event::IncomingMessage {
                    peer_id: Box::new(peer_id),
                    msg: Box::new(msg),
                },
                QueueKind::NetworkIncoming,
            )
            .await;
    }
    warn!(%our_id, %peer_id, "receiving handshake failed, closing connection");
    event_queue
        .schedule(
            Event::OutgoingFailed {
                peer_id: Box::new(Some(peer_id)),
                peer_address: Box::new(peer_address),
                error: Box::new(None),
            },
            QueueKind::Network,
        )
        .await
}

/// Network message reader.
///
/// Schedules all received messages until the stream is closed or an error occurs.
async fn message_reader<REv, P>(
    event_queue: EventQueueHandle<REv>,
    mut stream: SplitStream<FramedTransport<P>>,
    mut shutdown_receiver: watch::Receiver<()>,
    our_id: NodeId,
    peer_id: NodeId,
) -> io::Result<()>
where
    P: DeserializeOwned + Send + Display + Payload,
    REv: From<Event<P>>,
{
    let read_messages = async move {
        while let Some(msg_result) = stream.next().await {
            match msg_result {
                Ok(msg) => {
                    debug!(%our_id, %msg, %peer_id, "message received");
                    // We've received a message, push it to the reactor.
                    event_queue
                        .schedule(
                            Event::IncomingMessage {
                                peer_id: Box::new(peer_id),
                                msg: Box::new(msg),
                            },
                            QueueKind::NetworkIncoming,
                        )
                        .await;
                }
                Err(err) => {
                    warn!(%our_id, err=display_error(&err), %peer_id, "receiving message failed, closing connection");
                    return Err(err);
                }
            }
        }
        Ok(())
    };

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // while loop to terminate.
    match future::select(Box::pin(shutdown_messages), Box::pin(read_messages)).await {
        Either::Left(_) => info!(
            %our_id,
            %peer_id,
            "shutting down incoming connection message reader"
        ),
        Either::Right(_) => (),
    }

    Ok(())
}

/// Network message sender.
///
/// Reads from a channel and sends all messages, until the stream is closed or an error occurs.
///
/// Initially sends a handshake including the `chainspec_hash` as a final handshake step.  If the
/// recipient's `chainspec_hash` doesn't match, the connection will be closed.
async fn message_sender<P>(
    mut queue: UnboundedReceiver<Message<P>>,
    mut sink: SplitSink<FramedTransport<P>, Message<P>>,
    counter: IntGauge,
    handshake: Message<P>,
) -> Result<()>
where
    P: Serialize + Send + Payload,
{
    sink.send(handshake).await.map_err(Error::MessageNotSent)?;
    while let Some(payload) = queue.recv().await {
        counter.dec();
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
    CountingFormat<MessagePackFormat>,
>;

/// Constructs a new framed transport on a stream.
fn framed<P>(
    metrics: Arc<NetworkingMetrics>,
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

/// Initiates a TLS connection to a remote address.
async fn connect_outgoing(
    peer_address: SocketAddr,
    our_certificate: Arc<TlsCert>,
    secret_key: Arc<PKey<Private>>,
    server_is_stopped: Arc<AtomicBool>,
) -> Result<(NodeId, Transport)> {
    let ssl = tls::create_tls_connector(&our_certificate.as_x509(), &secret_key)
        .context("could not create TLS connector")?
        .configure()
        .and_then(|mut config| {
            config.set_verify_hostname(false);
            config.into_ssl("this-will-not-be-checked.example.com")
        })
        .map_err(Error::ConnectorConfiguration)?;

    let stream = TcpStream::connect(peer_address)
        .await
        .context("TCP connection failed")?;

    let mut tls_stream = SslStream::new(ssl, stream).context("tls handshake failed")?;
    SslStream::connect(Pin::new(&mut tls_stream)).await?;

    let peer_cert = tls_stream
        .ssl()
        .peer_certificate()
        .ok_or(Error::NoServerCertificate)?;

    let peer_id = tls::validate_cert(peer_cert)?.public_key_fingerprint();

    if server_is_stopped.load(Ordering::SeqCst) {
        debug!(
            our_id=%our_certificate.public_key_fingerprint(),
            %peer_address,
            "server stopped - aborting outgoing TLS connection"
        );
        Err(Error::ServerStopped)
    } else {
        Ok((NodeId::from(peer_id), tls_stream))
    }
}

impl<R, P> Debug for SmallNetwork<R, P>
where
    P: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmallNetwork")
            .field("our_id", &self.our_id)
            .field("certificate", &"<SSL cert>")
            .field("secret_key", &"<hidden>")
            .field("public_address", &self.public_address)
            .field("event_queue", &"<event_queue>")
            .field("incoming", &self.incoming)
            .field("outgoing", &self.outgoing)
            .field("pending", &self.pending)
            .finish()
    }
}
