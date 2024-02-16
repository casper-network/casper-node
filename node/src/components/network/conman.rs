//! Overlay network connection management.
//!
//! The core goal of this module is to allow the node to maintain a connection to other nodes on the
//! network, reconnecting on connection loss and ensuring there is always exactly one [`juliet`]
//! connection between peers.

use std::{
    collections::HashMap,
    fmt::Debug,
    net::IpAddr,
    sync::{Arc, RwLock},
    time::Instant,
};

use async_trait::async_trait;
use futures::FutureExt;
use juliet::rpc::{IncomingRequest, JulietRpcClient, RpcBuilder};
use strum::EnumCount;
use tokio::net::{TcpListener, TcpStream};
use tracing::{
    debug, error, error_span,
    field::{self, Empty},
    warn, Instrument, Span,
};

use crate::{
    types::NodeId,
    utils::{display_error, DropSwitch, ObservableFuse},
};

use super::{
    blocklist::BlocklistJustification, error::ConnectionError, handshake::HandshakeOutcome,
};

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
    ctx: Arc<ConManContext>,
    /// A fuse used to cancel future execution.
    shutdown: DropSwitch<ObservableFuse>,
}

/// Shared information across the connection manager and its subtasks.
struct ConManContext {
    /// Callback function to hand incoming requests off to.
    protocol_handler: Box<dyn ProtocolHandler>,
    /// Juliet RPC configuration.
    rpc_builder: RpcBuilder<{ super::Channel::COUNT }>,
    /// The shared state.
    state: RwLock<ConManState>,
}

/// Share state for [`ConMan`].
///
/// Tracks outgoing and incoming connections.
#[derive(Debug, Default)]
struct ConManState {
    /// A mapping of IP addresses that have been dialed, succesfully connected or backed off from.
    /// This is strictly for outgoing connections.
    // TODO: Add pruning for both tables, in case someone is flooding us with bogus addresses. We
    //       may need to add a queue for learning about new addresses.
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
    /// Connected to peer.
    Connected(PeerHandle),
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
    /// The established [`juliet`] RPC client, can be used to send requests to the peer.
    client: JulietRpcClient<{ super::Channel::COUNT }>,
}

#[async_trait]
pub(crate) trait ProtocolHandler: Send + Sync {
    async fn setup_incoming(
        &self,
        transport: TcpStream,
    ) -> Result<ProtocolHandshakeOutcome, ConnectionError>;

    async fn setup_outgoing(
        &self,
        transport: TcpStream,
    ) -> Result<ProtocolHandshakeOutcome, ConnectionError>;

    fn handle_incoming_request(&self, peer: NodeId, request: IncomingRequest);
}

pub(crate) struct ProtocolHandshakeOutcome {
    our_id: NodeId,
    peer_id: NodeId,
    handshake_outcome: HandshakeOutcome,
}

impl ConMan {
    /// Create a new connection manager.
    ///
    /// Immediately spawns a task accepting incoming connections on a tokio task, which will be
    /// cancelled if the returned [`ConMan`] is dropped.
    pub(crate) fn new<H: Into<Box<dyn ProtocolHandler>>>(
        listener: TcpListener,
        protocol_handler: H,
        rpc_builder: RpcBuilder<{ super::Channel::COUNT }>,
    ) -> Self {
        let ctx = Arc::new(ConManContext {
            protocol_handler: protocol_handler.into(),
            rpc_builder,
            state: Default::default(),
        });

        let shutdown = DropSwitch::new(ObservableFuse::new());

        let server_shutdown = shutdown.inner().clone();
        let server_ctx = ctx.clone();

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
                                .cancellable(handle_incoming(server_ctx.clone(), stream))
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

        Self { ctx, shutdown }
    }
}

/// Handler for a new incoming connection.
///
/// Will complete the handshake, then check if the incoming connection should be kept.
async fn handle_incoming(ctx: Arc<ConManContext>, stream: TcpStream) {
    let ProtocolHandshakeOutcome {
        our_id,
        peer_id,
        handshake_outcome,
    } = match ctx.protocol_handler.setup_incoming(stream).await {
        Ok(outcome) => outcome,
        Err(error) => {
            debug!(%error, "failed to complete setup TLS");
            return;
        }
    };

    // Register the `peer_id` and potential consensus key on the [`Span`] for logging from here on.
    Span::current().record("peer_id", &field::display(peer_id));
    if let Some(ref public_key) = handshake_outcome.peer_consensus_public_key {
        Span::current().record("consensus_key", &field::display(public_key));
    }

    if !incoming_preferred(our_id, peer_id) {
        // The connection is supposed to be outgoing from our perspective.
        // TODO: Learn aobut outgoing address.
        drop(handshake_outcome.public_addr); // TODO: Learn here instead.
        debug!("incoming connection, but outgoing connection preferred");

        // Drops the stream and thus closes the connection.
        return;
    }

    debug!("incoming connection established");

    // At this point, the initial connection negotiation is complete. Setup the `juliet` RPC transport, which we will need regardless to send errors.

    let (read_half, write_half) = tokio::io::split(handshake_outcome.transport);
    let (rpc_client, rpc_server) = ctx.rpc_builder.build(read_half, write_half);

    let mut rpc_server = {
        let mut guard = ctx.state.write().expect("lock poisoned");

        // Check if there already is a route registered.
        if let Some(existing) = guard.routing_table.get(&peer_id) {
            match existing {
                Route::Connected(_) => {
                    // We are already connected, meaning we got raced by another connection. Keep the
                    // existing and exit.
                    debug!("additional incoming connection ignored");
                    return;
                }
                Route::Banned {
                    until,
                    justification,
                } => {
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
            }
        }

        // At this point we are either replacing an expired ban or inserting a new entry.
        guard.routing_table.insert(
            peer_id,
            Route::Connected(PeerHandle {
                peer: peer_id,
                client: rpc_client,
            }),
        );

        rpc_server
    };

    loop {
        match rpc_server.next_request().await {
            Ok(Some(request)) => {
                // Incoming requests are directly handed off to the protocol handler.
                ctx.protocol_handler
                    .handle_incoming_request(peer_id, request);
            }
            Ok(None) => {
                // The connection was closed. Not an issue, the peer will need to reconnect to us.
                break;
            }
            Err(err) => {
                // TODO: this should not be a warning, downgrade to debug before shipping
                warn!(%err, "closing incoming connection due to error");
                break;
            }
        }
    }

    // Connection was closed, now update our state.
    let mut guard = ctx.state.write().expect("lock poisoned");
    match guard.routing_table.get(&peer_id) {
        Some(Route::Connected(_)) => {
            debug!("regular connection closure, expecting peer to reconnect");

            // Route is unchanged, remove it to ensure we can be reconnected to.
            guard.routing_table.remove(&peer_id);

            // TODO: Do we need to shut down the juliet clients? Likely not, if the
            // server is shut down?
        }
        Some(Route::Banned { .. }) => {
            // Leave the ban in place.
            debug!("connection closed and peer is banned");
        }
        None => {
            // This should not occur.
            error!("external source should never remove connection");
        }
    }
}

impl Debug for ConManContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConManContext")
            .field("protocol_handler", &"...")
            .field("rpc_builder", &"...")
            .field("state", &self.state)
            .finish()
    }
}

/// Determine whether we should prefer an incoming connection to the peer over an outgoing one.
///
/// Used to solve conflicts if two connections from/to the same peer are possible.
#[inline(always)]
fn incoming_preferred(our_id: NodeId, peer_id: NodeId) -> bool {
    our_id <= peer_id
}
