//! Management of outgoing connections.
//!
//! This module implements outgoing connection management, decoupled from the underlying transport
//! or any higher-level level parts. It encapsulates the reconnection and blocklisting logic on the
//! `SocketAddr` level.
//!
//! # Basic structure
//!
//! Core of this module is the `OutgoingManager`, which supports the following functionality:
//!
//! * Handed a `SocketAddr`s via the `learn_addr` function, it will permanently maintain a
//!   connection to the given address, only giving up if retry thresholds are exceeded, after which
//!   it will be forgotten.
//! * `block_addr` and `redeem_addr` can be used to maintain a `SocketAddr`-keyed block list.
//! * `OutgoingManager` maintains an internal routing table. The `get_route` function can be used to
//!   retrieve a "route" (typically a `sync::channel` accepting network messages) to a remote peer
//!   by `PeerId`.
//!
//! # Requirements
//!
//! `OutgoingManager` is decoupled from the underlying protocol, all of its interactions are
//! performed through a `Dialer`. This frees the `OutgoingManager` from having to worry about
//! protocol specifics, but requires a dialer to be present for most actions.
//!
//! Two conditions not expressed in code must be fulfilled for the `OutgoingManager` to function:
//!
//! * The `Dialer` is expected to produce `DialOutcomes` for every dial attempt eventually. These
//!   must be forwarded to the `OutgoingManager` via the `handle_dial_outcome` function.
//! * The `perform_housekeeping` method must be called periodically to give the the
//!   `OutgoingManager` a chance to initiate reconnections and collect garbage.

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    error::Error,
    fmt::{Debug, Display},
    mem,
    net::SocketAddr,
    time::{Duration, Instant},
};

use tracing::{debug, error, error_span, info, trace, warn, Span};

use super::{display_error, NodeId};

/// An outgoing connection/address in various states.
#[derive(Debug)]
struct Outgoing<P>
where
    P: Dialer,
{
    /// Whether or not the address is unforgettable, see `learn_addr` for details.
    is_unforgettable: bool,
    /// The current state the connection/address is in.
    state: OutgoingState<P>,
}

/// Active state for a connection/address.
#[derive(Debug)]
enum OutgoingState<P>
where
    P: Dialer,
{
    /// The outgoing address has been known for the first time and we are currently connecting.
    Connecting {
        /// Number of attempts that failed, so far.
        failures_so_far: u8,
    },
    /// The connection has failed at least one connection attempt and is waiting for a retry.
    Waiting {
        /// Number of attempts that failed, so far.
        failures_so_far: u8,
        /// The most recent connection error.
        error: P::Error,
        /// The precise moment when the last connection attempt failed.
        last_failure: Instant,
    },
    /// An established outgoing connection.
    Connected {
        /// The peers remote ID.
        peer_id: NodeId,
        /// Handle to a communication channel that can be used to send data to the peer.
        ///
        /// Can be a channel to decouple sending, or even a direct connection handle.
        handle: P::Handle,
    },
    /// The address was explicitly blocked and will not be retried.
    Blocked,
    /// The address is a loopback address, connecting to ourselves and will not be tried again.
    Loopback,
}

impl<P> OutgoingState<P>
where
    P: Dialer,
{
    fn reset() -> Self {
        OutgoingState::Connecting { failures_so_far: 0 }
    }
}

/// The result of dialing `SocketAddr`.
#[derive(Debug)]
pub(crate) enum DialOutcome<P>
where
    P: Dialer,
{
    /// A connection was successfully established.
    Successful {
        /// The address dialed.
        addr: SocketAddr,
        /// A handle to send data down the connection.
        handle: P::Handle,
        /// The remote peer's authenticated node ID.
        node_id: NodeId,
    },
    /// The connection attempt failed.
    Failed {
        /// The address dialed.
        addr: SocketAddr,
        /// The error encountered while dialing.
        error: P::Error,
        /// The moment the connection attempt failed.
        when: Instant,
    },
    /// The connection was aborted, because the remote peer turned out to be a loopback.
    Loopback {
        /// The address used to connect.
        addr: SocketAddr,
    },
}

impl<D> DialOutcome<D>
where
    D: Dialer,
{
    /// Retrieves the socket address from the `DialOutcome`.
    fn addr(&self) -> SocketAddr {
        match self {
            DialOutcome::Successful { addr, .. } => *addr,
            DialOutcome::Failed { addr, .. } => *addr,
            DialOutcome::Loopback { addr, .. } => *addr,
        }
    }
}

/// A connection dialer.
pub(crate) trait Dialer {
    /// The type of handle this dialer produces. This module does not interact with handles, but
    /// makes them available on request to other parts.
    type Handle: Debug;

    /// The error produced by the `Dialer` when a connection fails.
    type Error: Debug + Display + Error + Sized;

    /// Attempt to connect to the outgoing socket address.
    ///
    /// For every time this function is called, there must be a corresponding call to
    /// `handle_dial_outcome` eventually.
    ///
    /// The caller is responsible for ensuring that there is always only one `connect_outgoing` call
    /// that has not been answered by `handle_dial_outcome` for every `addr`.
    ///
    /// Any logging of connection issues should be done in the context of `span` for better log
    /// output.
    fn connect_outgoing(&self, span: Span, addr: SocketAddr);
}

#[derive(Debug)]
/// Connection settings for the outgoing connection manager.
pub(crate) struct OutgoingConfig {
    /// The maximum number of attempts before giving up and forgetting an address, if permitted.
    pub(crate) retry_attempts: u8,
    /// The basic time slot for exponential backoff when reconnecting.
    pub(crate) base_timeout: Duration,
}

impl Default for OutgoingConfig {
    fn default() -> Self {
        // The default configuration retries 12 times, over the course of a little over 30 minutes.
        OutgoingConfig {
            retry_attempts: 12,
            base_timeout: Duration::from_millis(500),
        }
    }
}

impl OutgoingConfig {
    /// Calculates the backoff time.
    ///
    /// `failed_attempts` (n) is the number of previous attempts *before* the current failure (thus
    /// starting at 0). The backoff time will be double for each attempt.
    fn calc_backoff(&self, failed_attempts: u8) -> Duration {
        2u32.pow(failed_attempts as u32) * self.base_timeout
    }
}

/// Manager of outbound connections.
///
/// See the module documentation for usage suggestions.
#[derive(Debug, Default)]
pub(crate) struct OutgoingManager<D>
where
    D: Dialer,
{
    /// Outgoing connections subsystem configuration.
    config: OutgoingConfig,
    /// Mapping of address to their current connection state.
    outgoing: HashMap<SocketAddr, Outgoing<D>>,
    /// Routing table.
    ///
    /// Contains a mapping from node IDs to connected socket addresses. A missing entry means that
    /// the destination is not connected.
    routes: HashMap<NodeId, SocketAddr>,
    /// A cache of addresses that are in the `Waiting` state, used during housekeeping.
    waiting_cache: HashSet<SocketAddr>,
}

impl<D> OutgoingManager<D>
where
    D: Dialer,
{
    /// Creates a logging span for a specific connection.
    #[inline]
    fn mk_span(&self, addr: SocketAddr) -> Span {
        // Note: The jury is still out on whether we want to create a single span per connection and
        // cache it, or create a new one (with the same connection ID) each time this is called. The
        // advantage of the former is external tools have it easier correlating all related
        // information, while the drawback is not being able to change the parent span link, which
        // might be awkward.

        if let Some(_outgoing) = self.outgoing.get(&addr) {
            error_span!("outgoing", %addr, state = "TODO")
        } else {
            error_span!("outgoing", %addr, state = "-")
        }
    }

    /// Updates internal caches after a state change.
    ///
    /// Given a potential previous state, updates all internal caches like `routes`, etc.
    fn update_caches(&mut self, addr: SocketAddr, prev_state: Option<&OutgoingState<D>>) {
        // Check if we need to update the routing table.
        let new_state = if let Some(outgoing) = self.outgoing.get(&addr) {
            &outgoing.state
        } else {
            error!("tried to update cache based on non-existent outgoing connection");
            return;
        };

        match (&prev_state, &new_state) {
            (Some(OutgoingState::Connected { .. }), OutgoingState::Connected { .. }) => {
                trace!("no change in routing, already connected");
            }

            // Dropping from connected to any other state requires clearing the route.
            (Some(OutgoingState::Connected { peer_id, .. }), _) => {
                debug!(%peer_id, "no more route for peer");
                self.routes.remove(peer_id);
            }

            // Otherwise we have established a new route.
            (_, OutgoingState::Connected { peer_id, .. }) => {
                debug!(%peer_id, "route added");
                self.routes.insert(peer_id.clone(), addr);
            }

            _ => {
                trace!("no change in routing");
            }
        }

        // Check if we need to consider the connection for reconnection on next sweep.
        match (&prev_state, &new_state) {
            (Some(OutgoingState::Waiting { .. }), OutgoingState::Waiting { .. }) => {
                trace!("no change in waiting state, already waiting");
            }
            (Some(OutgoingState::Waiting { .. }), _) => {
                self.waiting_cache.remove(&addr);
                debug!("waiting to reconnect");
            }
            (_, OutgoingState::Waiting { .. }) => {
                self.waiting_cache.remove(&addr);
                debug!("now reconnecting");
            }
            _ => {
                trace!("no change in waiting state");
            }
        }
    }

    /// Changes the state of an outgoing connection.
    ///
    /// Will trigger an update of the routing table if necessary.
    ///
    /// Calling this function on an unknown `addr` will emit an error but otherwise be ignored.
    fn change_outgoing_state(&mut self, addr: SocketAddr, mut new_state: OutgoingState<D>) {
        let prev_state = match self.outgoing.entry(addr) {
            Entry::Vacant(vacant) => {
                vacant.insert(Outgoing {
                    state: new_state,
                    is_unforgettable: false,
                });
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

    /// Retrieves a handle to a peer.
    ///
    /// Primary function to send data to peers; clients retrieve a handle to it which can then
    /// be used to send data.
    pub(crate) fn get_route(&self, peer_id: NodeId) -> Option<&D::Handle> {
        let outgoing = self.outgoing.get(self.routes.get(&peer_id)?)?;

        if let OutgoingState::Connected { ref handle, .. } = outgoing.state {
            Some(handle)
        } else {
            None
        }
    }

    /// Notify about a potentially new address that has been discovered.
    ///
    /// Immediately triggers the connection process to said address if it was not known before.
    ///
    /// A connection marked `unforgettable` will never be evicted but reset instead when it exceeds
    /// the retry limit.
    pub(crate) fn learn_addr(&mut self, proto: &mut D, addr: SocketAddr, unforgettable: bool) {
        let span = self.mk_span(addr);
        span.clone().in_scope(move || {
            match self.outgoing.entry(addr) {
                Entry::Occupied(_) => {
                    debug!("ignoring already known address");
                }
                Entry::Vacant(_vacant) => {
                    info!("connecting to newly learned address");
                    proto.connect_outgoing(span, addr);
                    self.change_outgoing_state(
                        addr,
                        OutgoingState::Connecting { failures_so_far: 0 },
                    );
                }
            };

            if unforgettable {
                if let Some(outgoing) = self.outgoing.get_mut(&addr) {
                    if !outgoing.is_unforgettable {
                        debug!("marked unforgettable");
                        outgoing.is_unforgettable = true;
                    }
                } else {
                    error!("tried to set unforgettable on lost address");
                }
            }
        })
    }

    /// Blocks an address.
    ///
    /// Causes any current connection to the address to be terminated and future ones prohibited.
    pub(crate) fn block_addr(&mut self, addr: SocketAddr) {
        let span = self.mk_span(addr);

        span.in_scope(move || match self.outgoing.entry(addr) {
            Entry::Vacant(_vacant) => {
                info!("address blocked");
                self.change_outgoing_state(addr, OutgoingState::Blocked);
            }
            // TODO: Check what happens on close on our end, i.e. can we distinguish in logs between
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
    pub(crate) fn redeem_addr(&mut self, proto: &mut D, addr: SocketAddr) {
        let span = self.mk_span(addr);
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Vacant(_) => {
                    debug!("ignoring redemption of unknown address");
                }
                Entry::Occupied(occupied) => match occupied.get().state {
                    OutgoingState::Blocked => {
                        proto.connect_outgoing(span, addr);
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Connecting { failures_so_far: 0 },
                        );
                    }
                    _ => {
                        debug!("ignoring redemption of address that is not blocked");
                    }
                },
            });
    }

    /// Performs housekeeping like reconnection, etc.
    ///
    /// This function must periodically be called. A good interval is every second or faster.
    fn perform_housekeeping(&mut self, proto: &mut D) {
        let mut corrupt_entries = Vec::new();
        let mut forgettable_entries = Vec::new();
        let mut reconnections = Vec::new();

        let config = &self.config;
        let now = Instant::now();

        for &addr in &self.waiting_cache {
            let span = self.mk_span(addr);
            let entry = self.outgoing.entry(addr);

            span.clone().in_scope(|| match entry {
                Entry::Vacant(_) => {
                    error!(%addr, "corrupt cache, missing entry");
                    corrupt_entries.push(addr);
                }
                Entry::Occupied(occupied) => {
                    let outgoing = occupied.into_mut();
                    match outgoing.state {
                        // Decide whether to attempt reconnecting a failed-waiting address.
                        OutgoingState::Waiting {
                            ref mut failures_so_far,
                            ref mut last_failure,
                            ..
                        } => {
                            if *failures_so_far >= config.retry_attempts {
                                if outgoing.is_unforgettable {
                                    // Unforgettable addresses simply have their timer reset.
                                    info!("resetting unforgettable address");

                                    reconnections.push((addr, 0));
                                } else {
                                    // Address had too many attempts at reconnection, we will forget
                                    // it later if forgettable.
                                    forgettable_entries.push(addr);

                                    info!("gave up on address");
                                }
                            } else {
                                // The address has not exceeded the limit, so check if it is due.
                                let due = *last_failure + config.calc_backoff(*failures_so_far);
                                if due >= now {
                                    debug!(attempts = *failures_so_far, "attempting reconnection");

                                    proto.connect_outgoing(span, addr);

                                    reconnections.push((addr, *failures_so_far + 1));
                                }
                            }
                        }
                        _ => {
                            error!(%addr, "corrupt cache, not in failed-waiting state");
                        }
                    }
                }
            });
        }

        // There should never be any corrupt entries, but we program defensively here.
        corrupt_entries.into_iter().for_each(|addr| {
            self.waiting_cache.remove(&addr);
        });

        // All entries that have expired can also be removed.
        forgettable_entries.iter().for_each(|addr| {
            self.outgoing.remove(addr);
            self.waiting_cache.remove(addr);
        });

        // Trigger reconnections for the unforgettables.
        reconnections
            .into_iter()
            .for_each(|(addr, failures_so_far)| {
                proto.connect_outgoing(self.mk_span(addr), addr);
                self.change_outgoing_state(addr, OutgoingState::Connecting { failures_so_far });
            })
    }

    /// Handles the outcome of a dialing attempt.
    ///
    /// Note that reconnects will earliest happen on the next `perform_housekeeping` call.
    pub(crate) fn handle_dial_outcome(&mut self, dial_outcome: DialOutcome<D>) {
        let span = self.mk_span(dial_outcome.addr());

        span.in_scope(move || match dial_outcome {
            DialOutcome::Successful {
                addr,
                handle,
                node_id,
                ..
            } => {
                info!("established outgoing connection");
                self.change_outgoing_state(
                    addr,
                    OutgoingState::Connected {
                        peer_id: node_id,
                        handle,
                    },
                );
            }
            DialOutcome::Failed { addr, error, when } => {
                info!(err = display_error(&error), "outgoing connection failed");
                if let Some(outgoing) = self.outgoing.get(&addr) {
                    if let OutgoingState::Waiting {
                        failures_so_far, ..
                    } = outgoing.state
                    {
                        // This is not the first connection failure for this address, so update the
                        // progress state information.
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Waiting {
                                failures_so_far: failures_so_far + 1,
                                error,
                                last_failure: when,
                            },
                        )
                    }
                } else {
                    self.change_outgoing_state(
                        addr,
                        OutgoingState::Waiting {
                            failures_so_far: 0,
                            error,
                            last_failure: when,
                        },
                    )
                }
            }
            DialOutcome::Loopback { addr } => {
                info!("found loopback address");
                self.change_outgoing_state(addr, OutgoingState::Loopback);
            }
        });
    }
}
