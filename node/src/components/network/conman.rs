//! Overlay network connection management.
//!
//! The core goal of this module is to allow the node to maintain a connection to other nodes on the
//! network, reconnecting on connection loss and ensuring there is always exactly one [`juliet`]
//! connection between peers.

// TODO: This module's core design of removing entries on drop is safe, but suboptimal, as it leads
//       to a lot of lock contention on drop. A careful redesign might ease this burden.

// TODO: Consider adding pruning for tables, in case someone is flooding us with bogus addresses.

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::{TryFuture, TryFutureExt};
use juliet::rpc::{IncomingRequest, JulietRpcClient, JulietRpcServer, RpcBuilder, RpcServerError};
use strum::EnumCount;
use thiserror::Error;
use tokio::{
    io::{ReadHalf, WriteHalf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError},
};
use tracing::{
    debug, error, error_span,
    field::{self, Empty},
    info, trace, warn, Instrument, Span,
};

use crate::{
    types::NodeId,
    utils::{display_error, once_per::rate_limited, DropSwitch, ObservableFuse},
};

use super::{
    blocklist::BlocklistJustification, error::ConnectionError, handshake::HandshakeOutcome,
    Transport,
};

type RpcClient = JulietRpcClient<{ super::Channel::COUNT }>;

type RpcServer =
    JulietRpcServer<{ super::Channel::COUNT }, ReadHalf<Transport>, WriteHalf<Transport>>;

/// Connection manager.
///
/// The connection manager accepts incoming connections and intiates outgoing connections upon
/// learning about new addresses. It also handles reconnections, disambiguation when there is both
/// an incoming and outgoing connection, and back-off timers for connection attempts.
///
/// `N` is the number of channels by the instantiated `juliet` protocol.
#[derive(Debug)]
pub(crate) struct ConMan {
    /// The shared connection manager state, which contains per-peer and per-address information.
    ctx: Arc<ConManContext>,
    /// A fuse used to cancel execution.
    ///
    /// Causes all background tasks (incoming, outgoing and server) to be shutdown as soon as
    /// `ConMan` is dropped.
    shutdown: DropSwitch<ObservableFuse>,
}

#[derive(Copy, Clone, Debug)]
/// Configuration settings for the connection manager.
struct Config {
    /// The timeout for one TCP to be connection to be established, from a single `connect` call.
    tcp_connect_timeout: Duration,
    /// How often to reattempt a connection.
    ///
    /// At one second, 8 attempts means that the last attempt will be delayed for 128 seconds.
    tcp_connect_attempts: usize,
    /// Base delay for the backoff, grows exponentially until `tcp_connect_attempts` maxes out.
    tcp_connect_base_backoff: Duration,
    /// How long to back off from reconnecting to an address after a failure that indicates a
    /// significant problem.
    significant_error_backoff: Duration,
    /// How long to back off from reconnecting to an address if the error is likely not going to
    /// change for a long time.
    permanent_error_backoff: Duration,
    /// How long to wait before attempting to reconnect when an outgoing connection is lost.
    reconnect_delay: Duration,
    /// Number of incoming connections before refusing to accept any new ones.
    max_incoming_connections: usize,
    /// Number of outgoing connections before stopping to connect to learned addresses.
    max_outgoing_connections: usize,
}

/// Shared information across the connection manager and its subtasks.
struct ConManContext {
    /// Shared configuration settings.
    cfg: Config,
    /// Callback handler for connection setup and incoming request handling.
    protocol_handler: Box<dyn ProtocolHandler>,
    /// Juliet RPC configuration.
    rpc_builder: RpcBuilder<{ super::Channel::COUNT }>,
    /// The shared state.
    state: RwLock<ConManState>,
    /// Our own address (for loopback filtering).
    public_addr: SocketAddr,
    /// Our own node ID.
    our_id: NodeId,
    /// Limiter for incoming connections.
    incoming_limiter: Arc<Semaphore>,
}

/// Share state for [`ConMan`].
///
/// Tracks outgoing and incoming connections.
#[derive(Debug, Default)]
struct ConManState {
    /// A set of outgoing address for which a handler is currently running.
    address_book: HashSet<SocketAddr>,
    /// Mapping of [`SocketAddr`]s to an instant in the future until which they must not be dialed.
    do_not_call: HashMap<SocketAddr, Instant>,
    /// The current route per node ID.
    ///
    /// An entry in this table indicates an established connection to a peer. Every entry in this
    /// table is controlled by an `OutgoingHandler`, all other access should be read-only.
    routing_table: HashMap<NodeId, Route>,
    /// A mapping of `NodeId`s to details about their bans.
    banlist: HashMap<NodeId, Sentence>,
}

/// Record of punishment for a peers malicious behavior.
#[derive(Debug)]
struct Sentence {
    /// Time until the ban is lifted.
    until: Instant,
    /// Justification for the ban.
    justification: BlocklistJustification,
}

/// Data related to an established connection.
#[derive(Debug)]
struct Route {
    /// Node ID of the peer.
    peer: NodeId,
    /// The established [`juliet`] RPC client that is used to send requests to the peer.
    client: RpcClient,
}

/// External integration.
///
/// Contains callbacks for transport setup (via [`setup_incoming`] and [`setup_outgoing`]) and
/// handling of actual incoming requests.
#[async_trait]
pub(crate) trait ProtocolHandler: Send + Sync {
    /// Sets up an incoming connection.
    ///
    /// Given a TCP stream of an incoming connection, should setup any higher level transport and
    /// perform a handshake.
    async fn setup_incoming(
        &self,
        stream: TcpStream,
    ) -> Result<ProtocolHandshakeOutcome, ConnectionError>;

    /// Sets up an outgoing connection.
    ///
    /// Given a TCP stream of an outgoing connection, should setup any higher level transport and
    /// perform a handshake.
    async fn setup_outgoing(
        &self,
        stream: TcpStream,
    ) -> Result<ProtocolHandshakeOutcome, ConnectionError>;

    /// Process one incoming request.
    fn handle_incoming_request(&self, peer: NodeId, request: IncomingRequest);
}

/// The outcome of a handshake performed by the [`ProtocolHandler`].
pub(crate) struct ProtocolHandshakeOutcome {
    /// Peer's `NodeId`.
    peer_id: NodeId,
    /// The actual handshake outcome.
    handshake_outcome: HandshakeOutcome,
}

impl ProtocolHandshakeOutcome {
    /// Registers the handshake outcome on the tracing span, to give context to logs.
    fn record_on(&self, span: Span) {
        span.record("peer_id", &field::display(self.peer_id));

        if let Some(ref public_key) = self.handshake_outcome.peer_consensus_public_key {
            span.record("consensus_key", &field::display(public_key));
        }
    }
}

impl ConMan {
    /// Create a new connection manager.
    ///
    /// Immediately spawns a task accepting incoming connections on a tokio task. The task will be
    /// stopped if the returned [`ConMan`] is dropped.
    pub(crate) fn new<H: Into<Box<dyn ProtocolHandler>>>(
        listener: TcpListener,
        public_addr: SocketAddr,
        our_id: NodeId,
        protocol_handler: H,
        rpc_builder: RpcBuilder<{ super::Channel::COUNT }>,
    ) -> Self {
        let cfg = Config::default();
        let ctx = Arc::new(ConManContext {
            cfg,
            protocol_handler: protocol_handler.into(),
            rpc_builder,
            state: Default::default(),
            public_addr,
            our_id,
            incoming_limiter: Arc::new(Semaphore::new(cfg.max_incoming_connections)),
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
                        let span =
                            error_span!("incoming", %peer_addr, peer_id=Empty, consensus_key=Empty);

                        match server_ctx.incoming_limiter.clone().try_acquire_owned() {
                            Ok(permit) => server_shutdown.spawn(
                                IncomingHandler::handle(
                                    server_ctx.clone(),
                                    stream,
                                    server_shutdown.clone(),
                                    permit,
                                )
                                .instrument(span),
                            ),
                            Err(TryAcquireError::NoPermits) => {
                                rate_limited!(
                                    EXCEED_INCOMING,
                                    |dropped| warn!(most_recent_skipped=%peer_addr, dropped, "exceeded incoming connection limit, are you getting spammed?")
                                );
                            }
                            Err(TryAcquireError::Closed) => {
                                // We may be shutting down.
                                debug!("incoming limiter semaphore closed");
                            }
                        }
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

        shutdown.inner().spawn(server);

        Self { ctx, shutdown }
    }
}

impl ConManContext {
    /// Informs the system about a potentially new address.
    ///
    /// Does a preliminary check whether or not a new outgoing handler should be spawn for the
    /// supplied `peer_address`. These checks are performed on a read lock to avoid write lock
    /// contention, but repeated by the spawned handler (if any are spawned) afterwards to avoid
    /// race conditions.
    fn learn_addr(self: Arc<Self>, peer_addr: SocketAddr, shutdown: ObservableFuse) {
        if peer_addr == self.public_addr {
            trace!("ignoring loopback address");
            return;
        }

        // We have been informed of a new address. Find out if it is new or uncallable.
        {
            let guard = self.state.read().expect("lock poisoned");

            let now = Instant::now();
            if guard.should_not_call(&peer_addr, now) {
                trace!(%peer_addr, "is on do-not-call list");
                return;
            }

            if guard.address_book.contains(&peer_addr) {
                // There already exists a handler attempting to connect, exit.
                trace!(%peer_addr, "discarding peer address, already has outgoing handler");
                return;
            }

            // If we exhausted our address book capacity, discard the address, we will have to wait
            // until some active connections time out.
            if guard.address_book.len() >= self.cfg.max_outgoing_connections {
                rate_limited!(
                    EXCEED_ADDRESS_BOOK,
                    |dropped| warn!(most_recent_lost=%peer_addr, dropped, "exceeding maximum number of outgoing connections, you may be getting spammed")
                );

                return;
            }
        }

        // Our initial check whether or not we can connect was succesful, spawn a handler.
        let span = error_span!("outgoing", %peer_addr, peer_id=Empty, consensus_key=Empty);
        trace!(%peer_addr, "learned about address");

        shutdown.spawn(OutgoingHandler::run(self, peer_addr).instrument(span));
    }

    /// Sets up an instance of the [`juliet`] protocol on a transport returned.
    fn setup_juliet(&self, transport: Transport) -> (RpcClient, RpcServer) {
        let (read_half, write_half) = tokio::io::split(transport);
        self.rpc_builder.build(read_half, write_half)
    }
}

impl ConManState {
    /// Determines if an address is on the do-not-call list.
    #[inline(always)]
    fn should_not_call(&self, addr: &SocketAddr, now: Instant) -> bool {
        if let Some(until) = self.do_not_call.get(addr) {
            now <= *until
        } else {
            false
        }
    }

    /// Unconditionally removes an address from the do-not-call list.
    #[inline(always)]
    fn prune_should_not_call(&mut self, addr: &SocketAddr) {
        self.do_not_call.remove(addr);
    }

    /// Determines if a peer is still banned.
    ///
    /// Returns `None` if the peer is NOT banned, its remaining sentence otherwise.
    #[inline(always)]
    fn is_still_banned(&self, peer: &NodeId, now: Instant) -> Option<&Sentence> {
        self.banlist.get(peer).filter(|entry| now <= entry.until)
    }

    /// Unban a peer.
    ///
    /// Can safely be called if the peer is not banned.
    #[inline(always)]
    fn unban(&mut self, peer: &NodeId) {
        self.banlist.remove(peer);
    }
}

/// Handler for incoming connections.
///
/// The existance of an [`IncomingHandler`] is tied to an entry in the `routing_table` in
/// [`ConManState`]; as long as the handler exists, there will be a [`Route`] present.
struct IncomingHandler {
    /// The context this handler is tied to.
    ctx: Arc<ConManContext>,
    /// ID of the peer connecting to us.
    peer_id: NodeId,
}

impl IncomingHandler {
    /// Handles an incoming connection by setting up, spawning an [`IncomingHandler`] on success.
    ///
    /// Will exit early and close the connection if it is a low-ranking connection.
    ///
    /// ## Cancellation safety
    ///
    /// This function is cancellation safe, if cancelled, the connection will be closed. In any case
    /// routing table will be cleaned up if it was altered.
    async fn handle(
        ctx: Arc<ConManContext>,
        stream: TcpStream,
        shutdown: ObservableFuse,
        _permit: OwnedSemaphorePermit,
    ) {
        debug!("handling new connection attempt");

        let ProtocolHandshakeOutcome {
            peer_id,
            handshake_outcome,
        } = match ctx
            .protocol_handler
            .setup_incoming(stream)
            .await
            .map(move |outcome| {
                outcome.record_on(Span::current());
                outcome
            }) {
            Ok(outcome) => outcome,
            Err(error) => {
                debug!(%error, "failed to complete handshake on incoming");
                return;
            }
        };

        if peer_id == ctx.our_id {
            // Loopback connection established.
            error!("should never complete an incoming loopback connection");
            return;
        }

        if we_should_be_outgoing(ctx.our_id, peer_id) {
            // The connection is supposed to be outgoing from our perspective.
            debug!("closing low-ranking incoming connection");

            // Conserve public address, but drop the stream early, so that when we learn, the
            // connection is hopefully already closed.
            let public_addr = handshake_outcome.public_addr;
            drop(handshake_outcome);

            // Note: This is the original "Magic Mike" functionality.
            ctx.learn_addr(public_addr, shutdown.clone());

            return;
        }

        debug!("high-ranking incoming connection established");

        // At this point, the initial connection negotiation is complete. Setup the `juliet` RPC
        // transport, which we will need regardless to send errors.
        let (rpc_client, rpc_server) = ctx.setup_juliet(handshake_outcome.transport);

        let incoming_handler = {
            let mut guard = ctx.state.write().expect("lock poisoned");

            // Check if the peer is still banned. If it isn't, ensure the banlist is cleared.
            let now = Instant::now();
            if let Some(entry) = guard.is_still_banned(&peer_id, now) {
                debug!(until=?entry.until, justification=%entry.justification, "peer is still banned");
                // TODO: Send a proper error using RPC client/server here (requires appropriate
                //       Juliet API). This would allow the peer to update its backoff timer.
                return;
            }
            guard.unban(&peer_id);

            // Check if there is a route registered, i.e. an incoming handler is already running.
            if guard.routing_table.contains_key(&peer_id) {
                // We are already connected, meaning we got raced by another connection. Keep
                // the existing and exit.
                debug!("additional incoming connection ignored");
                return;
            }

            // At this point we are becoming the new route for the peer.
            guard.routing_table.insert(
                peer_id,
                Route {
                    peer: peer_id,
                    client: rpc_client,
                },
            );

            // We are now connected, and the authority for this specific connection. Before
            // releasing the lock, instantiate `Self`. This ensures the routing state is always
            // updated correctly, since `Self` will remove itself from the routing table on drop.
            Self {
                ctx: ctx.clone(),
                peer_id,
            }
        };

        info!("now connected via incoming connection");
        incoming_handler.run(rpc_server).await;
    }

    /// Runs the incoming handler's main acceptance loop.
    async fn run(self, mut rpc_server: RpcServer) {
        loop {
            match rpc_server.next_request().await {
                Ok(Some(request)) => {
                    // Incoming requests are directly handed off to the protocol handler.
                    trace!(%request, "received incoming request");
                    self.ctx
                        .protocol_handler
                        .handle_incoming_request(self.peer_id, request);
                }
                Ok(None) => {
                    // The connection was closed. Not an issue, the peer should reconnect to us.
                    info!("lost incoming connection");
                    return;
                }
                Err(err) => {
                    // TODO: this should not be a warning, downgrade to info before shipping
                    warn!(%err, "closing incoming connection due to error");
                    return;
                }
            }
        }
    }
}

impl Drop for IncomingHandler {
    fn drop(&mut self) {
        // Connection was closed, we need to ensure our entry in the routing table gets released.
        let mut guard = self.ctx.state.write().expect("lock poisoned");
        match guard.routing_table.remove(&self.peer_id) {
            Some(_) => {
                // TODO: Do we need to shut down the juliet clients? Likely not, if the server is
                //       shut down? In other words, verify that if the `juliet` server has shut
                //       down, all the clients are invalidated.
            }
            None => {
                // This must never happen.
                error!("nothing but `IncomingHandler` should modifiy the routing table");
            }
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

#[derive(Debug)]
struct OutgoingHandler {
    ctx: Arc<ConManContext>,
    peer_addr: SocketAddr,
}

#[derive(Debug, Error)]
enum OutgoingError {
    #[error("exhausted TCP reconnection attempts")]
    ReconnectionAttemptsExhausted(#[source] ConnectionError),
    #[error("failed to complete handshake")]
    FailedToCompleteHandshake(#[source] ConnectionError),
    #[error("loopback encountered")]
    LoopbackEncountered,
    #[error("should be incoming connection")]
    ShouldBeIncoming,
    #[error("remote peer is banned")]
    EncounteredBannedPeer(Instant),
    #[error("found residual routing data")]
    ResidualRoute,
    #[error("RPC server error")]
    RpcServerError(RpcServerError),
}

impl OutgoingHandler {
    /// Creates a new outgoing handler.
    ///
    /// This should be the only method used to create new instances of `OutgoingHandler`, to
    /// preserve the invariant of all of them being registered in an address book.
    fn new(state: &mut ConManState, arc_ctx: Arc<ConManContext>, peer_addr: SocketAddr) -> Self {
        state.address_book.insert(peer_addr);
        Self {
            ctx: arc_ctx,
            peer_addr,
        }
    }

    /// Runs the outgoing handler.
    ///
    /// Will perform repeated connection attempts to `peer_addr`, controlled by the configuration
    /// settings on the context.
    async fn run(ctx: Arc<ConManContext>, peer_addr: SocketAddr) {
        debug!("spawned new outgoing handler");

        // Check if we should connect at all, then register in address book.
        let mut outgoing_handler = {
            let mut guard = ctx.state.write().expect("lock poisoned");

            if guard.address_book.contains(&peer_addr) {
                debug!("got raced by another outgoing handler, aborting");
                return;
            }

            let now = Instant::now();
            if guard.should_not_call(&peer_addr, now) {
                // This should happen very rarely, it requires a racing handler to complete and the
                // resulting do-not-call to expire all while this function was starting.
                debug!("address turned do-not-call");
                return;
            }
            guard.prune_should_not_call(&peer_addr);

            Self::new(&mut guard.state, ctx.clone(), peer_addr)
        };

        // We now enter a connection loop. After attempting to connect and serve, we either sleep
        // and repeat the loop, connecting again, or `break` with a do-not-call timer.
        let do_not_call_until = loop {
            match outgoing_handler.connect_and_serve().await {
                // Note: `connect_and_serve` will have updated the tracing span fields for us.
                Ok(()) => {
                    // Regular connection closure, i.e. without error.
                    // TODO: Currently, peers that have banned us will end up here. They need a
                    //       longer reconnection delay.
                    info!("lost connection");
                    tokio::time::sleep(ctx.cfg.reconnect_delay).await;
                }
                Err(OutgoingError::EncounteredBannedPeer(until)) => {
                    // We will not keep attempting to connect to banned peers, put them on the
                    // do-not-call list.
                    break until;
                }
                Err(OutgoingError::FailedToCompleteHandshake(err)) => {
                    debug!(%err, "failed to complete handshake");
                    break Instant::now() + ctx.cfg.significant_error_backoff;
                }
                Err(OutgoingError::LoopbackEncountered) => {
                    info!("found loopback");
                    break Instant::now() + ctx.cfg.permanent_error_backoff;
                }
                Err(OutgoingError::ReconnectionAttemptsExhausted(err)) => {
                    // We could not connect to the address, so we are going to forget it.
                    debug!(%err, "forgetting address after error");
                    return;
                }
                Err(OutgoingError::ResidualRoute) => {
                    error!("encountered residual route, this should not happen");
                    break Instant::now() + ctx.cfg.significant_error_backoff;
                }
                Err(OutgoingError::RpcServerError(err)) => {
                    warn!(%err, "encountered RPC error");
                    break Instant::now() + ctx.cfg.significant_error_backoff;
                }
                Err(OutgoingError::ShouldBeIncoming) => {
                    debug!("should be incoming connection");
                    break Instant::now() + ctx.cfg.permanent_error_backoff;
                }
            }
        };

        // Update the do-not-call list.
        {
            let mut guard = ctx.state.write().expect("lock poisoned");

            if guard.do_not_call.len() >= ctx.cfg.max_outgoing_connections {
                rate_limited!(EXCEEDED_DO_NOT_CALL, |dropped| warn!(
                    most_recent_skipped=%peer_addr,
                    dropped,
                    "did not outgoing address to do-not-call list, already at capacity"
                ));
            } else {
                guard.do_not_call.insert(peer_addr, do_not_call_until);
            }
        }

        // Release the slot.
        drop(outgoing_handler);
    }

    async fn connect_and_serve(&mut self) -> Result<(), OutgoingError> {
        let stream = retry_with_exponential_backoff(
            self.ctx.cfg.tcp_connect_attempts,
            self.ctx.cfg.tcp_connect_base_backoff,
            || connect(self.ctx.cfg.tcp_connect_timeout, self.peer_addr),
        )
        .await
        .map_err(OutgoingError::ReconnectionAttemptsExhausted)?;

        let ProtocolHandshakeOutcome {
            peer_id,
            handshake_outcome,
        } = self
            .ctx
            .protocol_handler
            .setup_outgoing(stream)
            .await
            .map_err(OutgoingError::FailedToCompleteHandshake)
            .map(move |outcome| {
                outcome.record_on(Span::current());
                outcome
            })?;

        if peer_id == self.ctx.our_id {
            return Err(OutgoingError::LoopbackEncountered);
        }

        if !we_should_be_outgoing(self.ctx.our_id, peer_id) {
            return Err(OutgoingError::ShouldBeIncoming);
        }

        let (rpc_client, mut rpc_server) = self.ctx.setup_juliet(handshake_outcome.transport);

        // Update routing and outgoing state.
        {
            let mut guard = self.ctx.state.write().expect("lock poisoned");

            let now = Instant::now();
            if let Some(entry) = guard.is_still_banned(&peer_id, now) {
                debug!(until=?entry.until, justification=%entry.justification, "outgoing connection reached banned peer");
                // TODO: Send a proper error using RPC client/server here.

                return Err(OutgoingError::EncounteredBannedPeer(entry.until));
            }
            guard.unban(&peer_id);

            if guard
                .routing_table
                .insert(
                    peer_id,
                    Route {
                        peer: peer_id,
                        client: rpc_client,
                    },
                )
                .is_some()
            {
                return Err(OutgoingError::ResidualRoute);
            }
        }

        // All shared state has been updated, we can now run the server loop.
        while let Some(request) = rpc_server
            .next_request()
            .await
            .map_err(OutgoingError::RpcServerError)?
        {
            trace!(%request, "received incoming request");
            self.ctx
                .protocol_handler
                .handle_incoming_request(peer_id, request);
        }

        // Regular connection closing.
        Ok(())
    }
}

impl Drop for OutgoingHandler {
    fn drop(&mut self) {
        // When being dropped, we relinquish exclusive control over the address book entry.
        let mut guard = self.ctx.state.write().expect("lock poisoned");
        if !guard.address_book.remove(&self.peer_addr) {
            error!("address book should not be modified by anything but outgoing handler");
        }
    }
}

/// Connects to given address.
///
/// Will cancel the connection attempt once `TCP_CONNECT_TIMEOUT` is hit.
async fn connect(timeout: Duration, addr: SocketAddr) -> Result<TcpStream, ConnectionError> {
    tokio::time::timeout(timeout, TcpStream::connect(addr))
        .await
        .map_err(|_elapsed| ConnectionError::TcpConnectionTimeout)?
        .map_err(ConnectionError::TcpConnection)
}

/// Retries a given future with an exponential backoff timer between retries.
async fn retry_with_exponential_backoff<Fut, F>(
    max_attempts: usize,
    base_backoff: Duration,
    mut f: F,
) -> Result<<Fut as TryFuture>::Ok, <Fut as TryFuture>::Error>
where
    Fut: TryFuture,
    F: FnMut() -> Fut,
{
    debug_assert!(max_attempts > 0);

    let mut failed_attempts = 0;

    loop {
        match f().into_future().await {
            Ok(v) => return Ok(v),
            Err(err) => {
                let backoff = 2u32.pow(failed_attempts as u32) * base_backoff;

                failed_attempts += 1;
                if failed_attempts >= max_attempts {
                    return Err(err);
                }

                trace!(
                    failed_attempts,
                    remaining = max_attempts - failed_attempts,
                    ?backoff,
                    "attempt failed, backing off"
                );

                tokio::time::sleep(backoff).await;
            }
        }
    }
}

/// Determines whether an outgoing connection from us outranks an incoming connection from them.
#[inline(always)]
fn we_should_be_outgoing(our_id: NodeId, peer_id: NodeId) -> bool {
    our_id > peer_id
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tcp_connect_timeout: Duration::from_secs(10),
            tcp_connect_attempts: 8,
            tcp_connect_base_backoff: Duration::from_secs(1),
            significant_error_backoff: Duration::from_secs(60),
            permanent_error_backoff: Duration::from_secs(60 * 60),
            reconnect_delay: Duration::from_secs(5),
            max_incoming_connections: 10_000,
            max_outgoing_connections: 10_000,
        }
    }
}
