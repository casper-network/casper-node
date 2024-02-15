//! Overlay network connection management.
//!
//! The core goal of this module is to allow the node to maintain a connection to other nodes on the
//! network, reconnecting on connection loss and ensuring there is always exactly one [`juliet`]
//! connection between peers.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, RwLock},
    time::Instant,
};

use futures::FutureExt;
use juliet::rpc::JulietRpcClient;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error_span, field::Empty, warn, Instrument};

use crate::{
    types::NodeId,
    utils::{display_error, DropSwitch, ObservableFuse},
};

use super::blocklist::BlocklistJustification;

/// Connection manager.
///
/// The connection manager accepts incoming connections and intiates outgoing connections upon
/// learning about new addresses. It also handles reconnections, disambiguation when there is both
/// an incoming and outgoing connection, and back-off timers for connection attempts.
///
/// `N` is the number of channels by the instantiated `juliet` protocol.
#[derive(Debug)]
struct ConMan<const N: usize> {
    /// The shared connection manager state, which contains per-peer and per-address information.
    state: Arc<RwLock<ConManState<N>>>,
    /// A fuse used to cancel future execution.
    shutdown: DropSwitch<ObservableFuse>,
}

/// Share state for [`ConMan`].
///
/// Tracks outgoing and incoming connections.
#[derive(Debug, Default)]
struct ConManState<const N: usize> {
    /// A mapping of IP addresses that have been dialed, succesfully connected or backed off from.
    /// This is strictly for outgoing connections.
    address_book: HashMap<IpAddr, AddressBookEntry>,
    /// The current state per node ID, i.e. whether it is connected through an incoming or outgoing
    /// connection, blocked or unknown.
    routing_table: HashMap<NodeId, Route<N>>,
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
enum Route<const N: usize> {
    /// Connected through an incoming connection (initated by peer).
    Incoming(PeerHandle<N>),
    /// Connected through an outgoing connection (initiated by us).
    Outgoing(PeerHandle<N>),
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
struct PeerHandle<const N: usize> {
    /// NodeId of the peer.
    peer: NodeId,
    /// The established [`juliet`] RPC client, can be used to send requests to the peer.
    client: JulietRpcClient<N>,
}

impl<const N: usize> ConMan<N> {
    /// Create a new connection manager.
    ///
    /// Immediately spawns a task accepting incoming connections on a tokio task, which will be
    /// cancelled if the returned [`ConMan`] is dropped.
    pub(crate) fn new(listener: TcpListener) -> Self {
        let state: Arc<RwLock<ConManState<N>>> = Default::default();
        let server_state = state.clone();
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

                        tokio::spawn(
                            handle_incoming(stream, server_state.clone()).instrument(span),
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

        let shutdown = DropSwitch::new(ObservableFuse::new());
        tokio::spawn(shutdown.inner().clone().cancellable(server).map(|_| ()));

        Self { state, shutdown }
    }
}

/// Handler for a new incoming connection.
///
/// Will complete the handshake, then check if the incoming connection should be kept.
async fn handle_incoming<const N: usize>(stream: TcpStream, state: Arc<RwLock<ConManState<N>>>) {}
