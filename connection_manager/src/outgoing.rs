use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    error::Error,
    mem,
    net::SocketAddr,
};

use tracing::{debug, error, error_span, info, trace, warn, Span};

use super::NodeId;

#[derive(Debug)]
struct Outgoing {
    // Note: This struct saves per-IP metadata. If nothing of this kind comes up, this wrapper can
    //       be removed.
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
    /// Mapping of address to their current connection state.
    outgoing: HashMap<SocketAddr, Outgoing>,
    /// Routing table.
    ///
    /// Contains a mapping from node IDs to connected socket addresses. A missing entry means that
    /// the destination is not connected.
    routes: HashMap<NodeId, SocketAddr>,
    // A cache of addresses that are in the `Connecting` state, used when housekeeping.
    cache_connecting: HashSet<SocketAddr>,
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

    /// Updates internal caches after a state change.
    ///
    /// Given a potential previous state, updates all internal caches like `routes`, etc.
    fn update_caches(&mut self, addr: SocketAddr, prev_state: Option<&OutgoingState>) {
        // Check if we need to update the routing table.
        let new_state = if let Some(outgoing) = self.outgoing.get(&addr) {
            &outgoing.state
        } else {
            error!("tryed to update cache based on non-existant outgoing connection");
            return;
        };

        match (&prev_state, &new_state) {
            (Some(OutgoingState::Connected { .. }), OutgoingState::Connected { .. }) => {
                trace!("no change in routing, already connected");
            }

            // Dropping from connected to any other state requires clearing the route.
            (Some(OutgoingState::Connected { peer_id }), _) => {
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

        // Check if we need to consider the connection for reconnection on next sweep.
        match (&prev_state, &new_state) {
            (Some(OutgoingState::Connecting), OutgoingState::Connecting) => {
                trace!("no change in connecting state, already connecting");
            }
            (Some(OutgoingState::Connecting), _) => {
                self.cache_connecting.remove(&addr);
                debug!("no longer considered connecting");
            }
            (_, OutgoingState::Connecting) => {
                self.cache_connecting.remove(&addr);
                debug!("now connecting");
            }
            _ => {
                trace!("no change in connecting state");
            }
        }
    }

    /// Changes the state of an outgoing connection.
    ///
    /// Will trigger an update of the routing table if necessary.
    ///
    /// Calling this function on an unknown `addr` will emit an error but otherwise be ignored.
    fn change_outgoing_state(&mut self, addr: SocketAddr, mut new_state: OutgoingState) {
        let prev_state = match self.outgoing.entry(addr) {
            Entry::Vacant(vacant) => {
                vacant.insert(Outgoing { state: new_state });
                None
            }

            Entry::Occupied(occupied) => {
                let prev = occupied.into_mut();

                // With the routing updated, we can finally exchange the states.
                mem::swap(&mut prev.state, &mut new_state);

                // `new_state` is actually the previous state here.
                Some(new_state)
            }
        };

        // We would love to call `update_caches` in the match arms above, but `.entry` unfortunately
        // borrows `self` mutably already.
        self.update_caches(addr, prev_state.as_ref());
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
    pub(crate) fn block_addr(&mut self, addr: SocketAddr) {
        let span = self.mk_span(addr);

        span.in_scope(move || match self.outgoing.entry(addr) {
            Entry::Vacant(_vacant) => {
                info!("address blocked");
                self.change_outgoing_state(addr, OutgoingState::Blocked);
            }
            // TOOD: Check what happens on close on our end, i.e. can we distinguish in logs between
            // a closed connection on our end vs one that failed?
            Entry::Occupied(occupied) => match occupied.get().state {
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

#[cfg(test)]
mod tests {
    #[test]
    fn dummy() {
        // TODO
    }
}
