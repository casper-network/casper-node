//! Overlay network connection management.
//!
//! The core goal of this module is to allow the node to maintain a connection to other nodes on the
//! network, reconnecting on connection loss and ensuring there is always exactly one [`juliet`]
//! connection between peers.

use std::{collections::HashMap, net::IpAddr, sync::RwLock, time::Instant};

use futures::Future;
use juliet::rpc::JulietRpcClient;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error_span, field::Empty, warn, Instrument};

use crate::{types::NodeId, utils::display_error};

use super::blocklist::BlocklistJustification;

/// Connection manager.
///
/// The connection manager accepts incoming connections and intiates outgoing connections upon
/// learning about new addresses. It also handles reconnections, disambiguation when there is both
/// an incoming and outgoing connection, and back-off timers for connection attempts.
///
/// `N` is the number of channels by the instantiated `juliet` protocol.
///
/// ## Usage
///
/// After constructing a new connection manager, the server process should be started using
/// `run_incoming`.
#[derive(Debug, Default)]
struct ConMan<const N: usize> {
    state: RwLock<ConManState<N>>,
}

#[derive(Debug, Default)]
struct ConManState<const N: usize> {
    address_book: HashMap<IpAddr, AddressBookEntry>,
    routing_table: HashMap<NodeId, Route<N>>,
}

#[derive(Debug)]
enum AddressBookEntry {
    Connecting,
    Outgoing { remote: NodeId },
    BackOff { until: Instant },
}

#[derive(Debug)]
enum Route<const N: usize> {
    Incoming(PeerHandle<N>),
    Outgoing(PeerHandle<N>),
    Banned {
        until: Instant,
        justification: BlocklistJustification,
    },
}

#[derive(Debug)]
struct PeerHandle<const N: usize> {
    peer: NodeId,
    client: JulietRpcClient<N>,
}

impl<const N: usize> ConMan<N> {
    /// Run the incoming server socket.
    fn run_incoming(&self, listener: TcpListener) -> impl Future<Output = ()> {
        async move {
            loop {
                // We handle accept errors here, since they can be caused by a temporary resource
                // shortage or the remote side closing the connection while it is waiting in
                // the queue.
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        // The span setup is used throughout the entire lifetime of the connection.
                        let span =
                            error_span!("incoming", %peer_addr, peer_id=Empty, consensus_key=Empty);

                        tokio::spawn(handle_incoming(stream).instrument(span));
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
        }
    }
}

async fn handle_incoming(stream: TcpStream) {}
