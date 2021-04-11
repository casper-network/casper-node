use std::{
    collections::{hash_map::Entry, HashMap},
    error::Error,
    mem,
    net::SocketAddr,
};

use tracing::{debug, error, error_span, info, trace, warn, Span};

use super::NodeId;

#[derive(Debug)]
struct Outgoing {
    // TODO: Can we remove this?
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
    Connected { peer_id: NodeId },
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
pub(crate) trait ProtocolHandler {
    fn connect_outgoing(&self, span: Span, addr: SocketAddr);
}

#[derive(Debug, Default)]
pub(crate) struct OutgoingManager {
    outgoing: HashMap<SocketAddr, Outgoing>,
    routes: HashMap<NodeId, SocketAddr>,
}

impl OutgoingManager {
    /// Creates a logging span for a specific connection.
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

    /// Changes the state of an outgoing connection.
    ///
    /// Will trigger an update of the routing table if necessary.
    ///
    /// Calling this function on an unknown `addr` will emit an error but otherwise be ignored.
    fn change_outgoing_state(&mut self, addr: SocketAddr, mut new_state: OutgoingState) {
        match self.outgoing.entry(addr) {
            Entry::Vacant(_) => {
                // This should never happen, so we want about it.
                error!("failed to change state, entry disappeared");
            }
            Entry::Occupied(occupied) => {
                let prev_state = &mut occupied.into_mut().state;
                // Check if we need to update the routing table.
                match (&prev_state, &new_state) {
                    // Dropping from connected to any other state requires clearing the route.
                    (OutgoingState::Connected { peer_id }, _) => {
                        debug!(%peer_id, "no more route for peer");
                        self.routes.remove(peer_id);
                    }

                    // Otherwise we have established a new route.
                    (_, OutgoingState::Connected { peer_id }) => {
                        debug!(%peer_id, "route added");
                        self.routes.insert(peer_id.clone(), addr);
                    }

                    _ => {
                        trace!("no change in routing");
                    }
                }

                // With the routing updated, we can finally exchange the states.
                mem::swap(prev_state, &mut new_state);
            }
        }
    }

    /// Notify about a potentially new address that has been discovered.
    ///
    /// Immediately triggers the connection process to said address if it was not known before.
    pub(crate) fn learn_addr(&mut self, proto: &mut dyn ProtocolHandler, addr: SocketAddr) {
        let span = self.mk_span(addr);
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Occupied(_) => {
                    debug!("ignoring already known address");
                }
                Entry::Vacant(vacant) => {
                    info!("connecting to newly learned address");
                    proto.connect_outgoing(span, addr);
                    self.change_outgoing_state(addr, OutgoingState::Connecting);
                }
            })
    }

    /// Blocks an address.
    ///
    /// Causes all current connection to the address to be terminated and future ones prohibited.
    fn block_addr(&mut self, addr: SocketAddr) {
        let span = self.mk_span(addr);

        span.in_scope(move || match self.outgoing.entry(addr) {
            Entry::Vacant(vacant) => {
                info!("address blocked");
                self.change_outgoing_state(addr, OutgoingState::Blocked);
            }
            // TOOD: Check what happens on close on our end, i.e. can we distinguish in logs between
            // a closed connection on our end vs one that failed?
            Entry::Occupied(mut occupied) => match occupied.get().state {
                OutgoingState::Blocked => {
                    debug!("already blocking address");
                }
                OutgoingState::Loopback => {
                    warn!("requested to block ourselves, refusing to do so");
                }
                _ => {
                    info!("address blocked");
                    self.change_outgoing_state(addr, OutgoingState::Blocked);
                }
            },
        });
    }

    /// Removes an address from the block list.
    ///
    /// Does nothing if the address was not blocked.
    pub(crate) fn redeem_addr(&mut self, proto: &mut dyn ProtocolHandler, addr: SocketAddr) {
        let span = self.mk_span(addr);
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Vacant(_) => {
                    debug!("ignoring redemption of unknown address");
                }
                Entry::Occupied(occupied) => match occupied.get().state {
                    OutgoingState::Blocked => {
                        proto.connect_outgoing(span, addr);
                        self.change_outgoing_state(addr, OutgoingState::Connecting);
                    }
                    _ => {
                        debug!("ignoring redemption of address that is not blocked");
                    }
                },
            });
    }

    /// Performs housekeeping like reconnection, etc.
    ///
    /// This function must periodically be called. A good interval is every second.
    fn perform_housekeeping(&mut self) {
        todo!()
    }

    fn handle_event(&mut self, connection_outcome: ConnectionOutcome) {
        todo!()
    }
}
