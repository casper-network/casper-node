//! Overlay network connection management.
//!
//! The core goal of this module is to allow the node to maintain a connection to other nodes on the
//! network, reconnecting on connection loss and ensuring there is always exactly one [`juliet`]
//! connection between peers.

use std::{
    collections::HashMap,
    fmt::Display,
    net::IpAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::Instant,
};

use futures::FutureExt;
use juliet::rpc::JulietRpcClient;
use serde::{Deserialize, Serialize};
use strum::EnumCount;
use tokio::net::{TcpListener, TcpStream};
use tracing::{
    debug, error_span,
    field::{self, Empty},
    info, warn, Instrument, Span,
};

use crate::{
    components::network::{
        connection_id::ConnectionId, handshake::negotiate_handshake, tasks::server_setup_tls,
    },
    types::NodeId,
    utils::{display_error, DropSwitch, ObservableFuse},
};

use super::{blocklist::BlocklistJustification, tasks::NetworkContext, MessageKind, Payload};

/// Connection manager.
///
/// The connection manager accepts incoming connections and intiates outgoing connections upon
/// learning about new addresses. It also handles reconnections, disambiguation when there is both
/// an incoming and outgoing connection, and back-off timers for connection attempts.
///
/// `N` is the number of channels by the instantiated `juliet` protocol.
#[derive(Debug)]
struct ConMan {
    /// The shared connection manager state, which contains per-peer and per-address information.
    state: Arc<RwLock<ConManState>>,
    /// A fuse used to cancel future execution.
    shutdown: DropSwitch<ObservableFuse>,
}

/// Share state for [`ConMan`].
///
/// Tracks outgoing and incoming connections.
#[derive(Debug, Default)]
struct ConManState {
    /// A mapping of IP addresses that have been dialed, succesfully connected or backed off from.
    /// This is strictly for outgoing connections.
    address_book: HashMap<IpAddr, AddressBookEntry>,
    /// The current state per node ID, i.e. whether it is connected through an incoming or outgoing
    /// connection, blocked or unknown.
    routing_table: HashMap<NodeId, Route>,
}

/// An entry in the address book.
#[derive(Debug)]
enum AddressBookEntry {
    /// There currently is a task in charge of this outgoing address and trying to establish a
    /// connection.
    Connecting,
    /// An outgoing connection has been established to the given address.
    Outgoing {
        /// The node ID of the peer we are connected to at this address.
        remote: NodeId,
    },
    /// A decision has been made to not reconnect to the given address for the time being.
    BackOff {
        /// When to clear the back-off state.
        until: Instant,
    },
    // TODO: Consider adding `Incoming` as a hint to look up before attempting to connect.
}

/// A route to a peer.
#[derive(Debug)]
enum Route {
    /// Connected through an incoming connection (initated by peer).
    Incoming(PeerHandle),
    /// Connected through an outgoing connection (initiated by us).
    Outgoing(PeerHandle),
    /// The peer ID has been banned.
    Banned {
        /// Time ban is lifted.
        until: Instant,
        /// Justification for the ban.
        justification: BlocklistJustification,
    },
}

/// Data related to an established connection.
#[derive(Debug)]
struct PeerHandle {
    /// NodeId of the peer.
    peer: NodeId,
    /// The ID of the task handling this connection.
    task_id: TaskId,
    /// The established [`juliet`] RPC client, can be used to send requests to the peer.
    client: JulietRpcClient<{ super::Channel::COUNT }>,
}

impl ConMan {
    /// Create a new connection manager.
    ///
    /// Immediately spawns a task accepting incoming connections on a tokio task, which will be
    /// cancelled if the returned [`ConMan`] is dropped.
    pub(crate) fn new<REv: Send>(ctx: Arc<NetworkContext<REv>>, listener: TcpListener) -> Self {
        let state: Arc<RwLock<ConManState>> = Default::default();
        let shutdown = DropSwitch::new(ObservableFuse::new());

        let server_state = state.clone();
        let server_shutdown = shutdown.inner().clone();

        let server = async move {
            loop {
                // We handle accept errors here, since they can be caused by a temporary resource
                // shortage or the remote side closing the connection while it is waiting in
                // the queue.
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        // The span setup is used throughout the entire lifetime of the connection.
                        let span = error_span!("incoming", %peer_addr, peer_id=Empty, consensus_key=Empty, task_id=Empty);

                        tokio::spawn(
                            server_shutdown
                                .clone()
                                .cancellable(handle_incoming(
                                    ctx.clone(),
                                    stream,
                                    server_state.clone(),
                                ))
                                .instrument(span),
                        );
                    }

                    // TODO: Handle resource errors gracefully. In general, two kinds of errors
                    //       occur here: Local resource exhaustion, which should be handled by
                    //       waiting a few milliseconds, or remote connection errors, which can be
                    //       dropped immediately.
                    //
                    //       The code in its current state will consume 100% CPU if local resource
                    //       exhaustion happens, as no distinction is made and no delay introduced.
                    Err(ref err) => {
                        warn!(
                            ?listener,
                            err = display_error(err),
                            "dropping incoming connection during accept"
                        )
                    }
                }
            }
        };

        tokio::spawn(shutdown.inner().clone().cancellable(server).map(|_| ()));

        Self { state, shutdown }
    }
}

impl Route {
    /// Returns the task ID contained in the route.
    ///
    /// If there is no task ID found in `self`, returns 0.
    fn task_id(&self) -> TaskId {
        match self {
            Route::Incoming(_) => todo!(),
            Route::Outgoing(PeerHandle { task_id, .. }) => *task_id,
            // There is no task running, so return ID 0.
            Route::Banned {
                until,
                justification,
            } => TaskId::invalid(),
        }
    }
}

/// Handler for a new incoming connection.
///
/// Will complete the handshake, then check if the incoming connection should be kept.
async fn handle_incoming<REv>(
    context: Arc<NetworkContext<REv>>,
    stream: TcpStream,
    state: Arc<RwLock<ConManState>>,
) {
    let task_id = TaskId::unique();
    Span::current().record("task_id", u64::from(task_id));

    let (peer_id, transport) = match server_setup_tls(&context, stream).await {
        Ok(value) => value,
        Err(error) => {
            debug!(%error, "failed to complete setup TLS");
            return;
        }
    };

    // Register the `peer_id` on the [`Span`] for logging the ID from here on out.
    Span::current().record("peer_id", &field::display(peer_id));

    if peer_id == context.our_id {
        info!("incoming loopback connection");
        // We do not need to do anything here - the other end (outgoing) will detect this, then set
        // an appropriate backoff.
        return;
    }

    debug!("Incoming TLS connection established");

    // Setup connection id.
    let connection_id = ConnectionId::from_connection(transport.ssl(), context.our_id, peer_id);

    // Negotiate the handshake.
    let outcome =
        match negotiate_handshake::<DummyPayload, _>(&context, transport, connection_id).await {
            Ok(success) => success,
            Err(error) => {
                debug!(%error, "handshake failed");
                return;
            }
        };

    // We can now record the consensus key on the span.
    if let Some(ref public_key) = outcome.peer_consensus_public_key {
        Span::current().record("consensus_key", &field::display(public_key));
    }

    // At this point, the initial connection negotiation is complete. Setup the `juliet` RPC transport, which we will need regardless to send errors.

    let (read_half, write_half) = tokio::io::split(outcome.transport);
    let (rpc_client, rpc_server) = context.rpc_builder.build(read_half, write_half);

    let rpc_server = {
        let guard = state.write().expect("lock poisoned");

        match guard.routing_table.get(&peer_id) {
            Some(Route::Incoming(_)) => {
                // We received an additional incoming connection, this should not be happening with
                // well-behaved clients, unless there's a race in the underlying network layer.
                // We'll disconnect and rely on timeouts to clean up.
                debug!("ignoring additional incoming connection");
                return;
            }
            Some(Route::Outgoing(_)) => {
                todo!("disambiguate");
            }
            Some(Route::Banned {
                until,
                justification,
            }) => {
                let now = Instant::now();
                if now <= *until {
                    debug!(?until, %justification, "peer is still banned");
                    // TODO: Send a proper error using RPC client/server here (requires appropriate
                    //       Juliet API).
                    drop(rpc_client);
                    drop(rpc_server);
                    return;
                }
            }
            None => {
                // Fresh connection, just insert the rpc client.
                guard.routing_table.insert(
                    peer_id,
                    Route::Incoming(PeerHandle {
                        peer: peer_id,
                        task_id,
                        client: rpc_client,
                    }),
                );
            }
        }

        rpc_server
    };

    loop {
        match rpc_server.next_request().await {
            Ok(None) => {
                // The connection was closed. Not an issue, the peer will likely reconnect to us.
                let guard = state.write().expect("lock poisoned");

                match guard.routing_table.get(&peer_id) {
                    Some(route) if route.task_id() == task_id => {
                        debug!("regular connection closure, expecting peer to reconnect");
                        // Route is unchanged, we need to remove it to ensure we can be
                        // reconnected to.
                        guard.routing_table.remove(&peer_id);
                        // TODO: Do we need to shut down the juliet clients? Likely not, if the
                        // server is shut down?
                    }
                    _ => {
                        debug!("connection was already replaced");
                        // We are no longer in charge of maintaining the entry, just shut down.
                    }
                }

                return;
            }
            Ok(Some(incoming_request)) => {
                // TODO: Fire off request to somewhere.
            }
            Err(err) => {
                debug!(%err, "closing exiting incoming due to error");
                let guard = state.write().expect("lock poisoned");

                match guard.routing_table.get(&peer_id) {
                    Some(route) if route.task_id() == task_id => {
                        debug!(%err, "closing connection due to juliet error");
                        guard.routing_table.remove(&peer_id);
                        // TODO: Do we need to shut down the juliet clients? Likely not, if the
                        // server is shut down?
                    }
                    _ => {
                        debug!("cleaning up incoming that was already replaced");
                    }
                }

                return;
            }
        }
    }
}

/// Dummy payload for handshake negotiation.
///
/// The handshaking functions are currently tied to a specific payload, even though they don't need
/// to be for any other reason than backwards compatibility. This dummy payload is used instead of
/// the real node payload, it should be binary-compatible with older versions if only used for
/// handshaking purposes.
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
struct DummyPayload;

impl Payload for DummyPayload {
    fn message_kind(&self) -> MessageKind {
        MessageKind::Other
    }

    fn get_channel(&self) -> super::Channel {
        super::Channel::Network
    }
}

impl Display for DummyPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DummyPayload")
    }
}

/// A unique identifier for a task.
///
/// Similar to `tokio::task::TaskId` (which is unstable), but "permanently" unique.
///
/// Every task that is potentially long running and manages the routing table gets assigned a unique
/// task ID (through [`TaskId::unique`]), to allow the task itself to check if its routing entry has
/// been stolen or not.
#[derive(Copy, Clone, Debug)]
struct TaskId(u64);

impl TaskId {
    /// Returns the "invalid" TaskId, which is never equal to any other TaskId.
    fn invalid() -> TaskId {
        TaskId(0)
    }

    /// Generates a new task ID.
    fn unique() -> TaskId {
        static COUNTER: AtomicU64 = AtomicU64::new(1);

        TaskId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl From<TaskId> for u64 {
    #[inline(always)]
    fn from(value: TaskId) -> Self {
        value.0
    }
}

impl PartialEq for TaskId {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        self.0 != 0 && self.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::TaskId;

    #[test]
    fn task_id() {
        let a = TaskId::unique();
        let b = TaskId::unique();

        assert_ne!(a, TaskId::invalid());
        assert_ne!(b, TaskId::invalid());
        assert_ne!(TaskId::invalid(), TaskId::invalid());
        assert_ne!(a, b);
        assert_eq!(a, a);
        assert_eq!(b, b);
    }
}
