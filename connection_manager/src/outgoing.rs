use std::{collections::hash_map::Entry, net::SocketAddr};
use std::{collections::HashMap, error::Error};

use tracing::{debug, error_span, info, Span};

use super::NodeId;

#[derive(Debug)]
struct Outgoing {
    addr: SocketAddr,
    state: OutgoingState,
}

#[derive(Debug)]
enum OutgoingState {
    /// The outgoing address is known and we are currently connecting.
    Connecting,
    /// The connection has failed and is waiting for a retry.
    Failed,
    /// Functional outgoing connection.
    Connected,
    /// The address was blocked and will not be retried.
    Blocked,
    /// The address is a loopback address, connecting to ourselves and will not be tried again.
    Loopback,
}

enum ConnectionOutcome {
    Successful {
        addr: SocketAddr,
        node_id: Box<NodeId>,
    },
    Failed {
        addr: SocketAddr,
        error: Option<Box<dyn Error>>,
    },
}

// TODO: Rename me.
pub(crate) trait OutgoingConnectionHandler {
    fn connect_outgoing(&self, span: Span, addr: SocketAddr);
}

#[derive(Debug, Default)]
pub(crate) struct OutgoingManager {
    outgoing: HashMap<SocketAddr, Outgoing>,
    routes: HashMap<NodeId, SocketAddr>,
}

impl OutgoingManager {
    #[inline]
    fn mk_span(&self, addr: SocketAddr) -> Span {
        // Note: The jury is still out on whether we want to create a single span and cache it, or
        // create a new one each time this is called. The advantage of the former is external tools
        // being able to easier correlate all related information, while the drawback is not being
        // able to change the parent information.

        if let Some(_outgoing) = self.outgoing.get(&addr) {
            error_span!("outgoing", %addr, state = "TODO")
        } else {
            error_span!("outgoing", %addr, state = "-")
        }
    }

    /// Notify about a potentially new address that has been discovered.
    ///
    /// This will immediately trigger the connection process to said node, if the address was not
    /// known before.
    pub(crate) fn learn_addr(&mut self, ch: &mut dyn OutgoingConnectionHandler, addr: SocketAddr) {
        let span = self.mk_span(addr);
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Occupied(_) => {
                    debug!("ignoring already known addr");
                }
                Entry::Vacant(vacant) => {
                    info!("commencing connection (TODO: move to external?)");
                    ch.connect_outgoing(span, addr);
                    vacant.insert(Outgoing {
                        addr,
                        state: OutgoingState::Connecting,
                    });
                }
            })
    }

    fn block_addr(&mut self, addr: SocketAddr) {
        todo!()
    }

    fn clear_addr(&mut self, addr: SocketAddr) {
        todo!()
    }

    fn perform_housekeeping(&mut self) {
        todo!()
    }

    fn handle_event(&mut self, connection_outcome: ConnectionOutcome) {
        todo!()
    }
}
