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
//!   by `NodeId`.
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
//! * When a connection is dropped, the connection manager must be notified via
//!   `handle_connection_drop`.
//!
//! # Lifecycle
//!
//! The following chart illustrates the lifecycle of an outgoing connection.
//!
//! ```text
//!                            learn
//!                          ┌──────────────  unknown/forgotten
//!                          │ ┌───────────►  (implicit state)
//!                          │ │
//!                          │ │ exceed fail  │
//!                          │ │ limit        │ block
//!                          │ │              │
//!                          │ │              │
//!                          │ │              ▼
//!     ┌─────────┐          │ │        ┌─────────┐
//!     │         │    fail  │ │  block │         │
//!     │ Waiting │◄───────┐ │ │ ┌─────►│ Blocked │◄───────────┐
//! ┌───┤         │        │ │ │ │      │         │            │
//! │   └────┬────┘        │ │ │ │      └────┬────┘            │
//! │ block  │             │ │ │ │           │                 │
//! │        │ timeout     │ ▼ │ │           │ redeem,         │
//! │        │        ┌────┴─────┴───┐       │ block timeout   │
//! │        │        │              │       │                 │
//! │        └───────►│  Connecting  │◄──────┘                 │
//! │                 │              │                         │
//! │                 └─────┬────┬───┘                         │
//! │                       │ ▲  │                             │
//! │               success │ │  │ detect                      │
//! │                       │ │  │      ┌──────────┐           │
//! │ ┌───────────┐         │ │  │      │          │  block    │
//! │ │           │◄────────┘ │  │      │ Loopback ├───────────┤
//! │ │ Connected │           │  └─────►│          │           │
//! │ │           │ dropped   │         └──────────┘           │
//! │ └─────┬─────┴───────────┘                                │
//! │       │                                                  │
//! │       │ block                                            │
//! └───────┴──────────────────────────────────────────────────┘
//! ```

use std::{
    collections::{hash_map::Entry, HashMap},
    error::Error,
    fmt::{self, Debug, Display, Formatter},
    mem,
    net::SocketAddr,
    time::Duration,
};

#[cfg(test)]
use fake_instant::FakeClock as Instant;
#[cfg(not(test))]
use std::time::Instant;

use tracing::{debug, error_span, info, trace, warn, Span};

use super::{display_error, NodeId};

/// An outgoing connection/address in various states.
#[derive(Debug)]
struct Outgoing<D>
where
    D: Dialer,
{
    /// Whether or not the address is unforgettable, see `learn_addr` for details.
    is_unforgettable: bool,
    /// The current state the connection/address is in.
    state: OutgoingState<D>,
}

/// Active state for a connection/address.
#[derive(Debug)]
enum OutgoingState<D>
where
    D: Dialer,
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
        error: D::Error,
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
        handle: D::Handle,
    },
    /// The address was blocked and will not be retried.
    Blocked { since: Instant },
    /// The address is a loopback address, connecting to ourselves and will not be tried again.
    Loopback,
}

impl<D> Display for OutgoingState<D>
where
    D: Dialer,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OutgoingState::Connecting { failures_so_far } => {
                write!(f, "connecting({})", failures_so_far)
            }
            OutgoingState::Waiting {
                failures_so_far, ..
            } => write!(f, "waiting({})", failures_so_far),
            OutgoingState::Connected { .. } => write!(f, "connected"),
            OutgoingState::Blocked { .. } => write!(f, "blocked"),
            OutgoingState::Loopback => write!(f, "loopback"),
        }
    }
}

/// The result of dialing `SocketAddr`.
#[derive(Debug)]
pub(crate) enum DialOutcome<D>
where
    D: Dialer,
{
    /// A connection was successfully established.
    Successful {
        /// The address dialed.
        addr: SocketAddr,
        /// A handle to send data down the connection.
        handle: D::Handle,
        /// The remote peer's authenticated node ID.
        node_id: NodeId,
    },
    /// The connection attempt failed.
    Failed {
        /// The address dialed.
        addr: SocketAddr,
        /// The error encountered while dialing.
        error: D::Error,
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
    type Handle: Clone + Debug;

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

    /// Disconnects a potentially existing connection.
    ///
    /// Used when a peer has been blocked or should be disconnected for other reasons.
    fn disconnect_outgoing(&self, span: Span, handle: Self::Handle);
}

#[derive(Debug)]
/// Connection settings for the outgoing connection manager.
pub(crate) struct OutgoingConfig {
    /// The maximum number of attempts before giving up and forgetting an address, if permitted.
    pub(crate) retry_attempts: u8,
    /// The basic time slot for exponential backoff when reconnecting.
    pub(crate) base_timeout: Duration,
    /// Time until an outgoing address is unblocked.
    pub(crate) unblock_after: Duration,
}

impl Default for OutgoingConfig {
    fn default() -> Self {
        OutgoingConfig {
            retry_attempts: 12,
            base_timeout: Duration::from_millis(500),
            unblock_after: Duration::from_secs(600),
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
}

impl<D> OutgoingManager<D>
where
    D: Dialer,
{
    /// Creates a new outgoing manager.
    pub(crate) fn new(config: OutgoingConfig) -> Self {
        Self {
            config,
            outgoing: Default::default(),
            routes: Default::default(),
        }
    }
}

/// Creates a logging span for a specific connection.
#[inline]
fn mk_span<D: Dialer>(addr: SocketAddr, outgoing: Option<&Outgoing<D>>) -> Span {
    // Note: The jury is still out on whether we want to create a single span per connection and
    // cache it, or create a new one (with the same connection ID) each time this is called. The
    // advantage of the former is external tools have it easier correlating all related
    // information, while the drawback is not being able to change the parent span link, which
    // might be awkward.

    if let Some(outgoing) = outgoing {
        match outgoing.state {
            OutgoingState::Connected { peer_id, .. } => {
                error_span!("outgoing", %addr, state=%outgoing.state, %peer_id)
            }
            _ => error_span!("outgoing", %addr, state=%outgoing.state),
        }
    } else {
        error_span!("outgoing", %addr, state = "-")
    }
}

impl<D> OutgoingManager<D>
where
    D: Dialer,
{
    /// Changes the state of an outgoing connection.
    ///
    /// Will trigger an update of the routing table if necessary. Does not emit any other
    /// side-effects.
    fn change_outgoing_state(
        &mut self,
        addr: SocketAddr,
        mut new_state: OutgoingState<D>,
    ) -> &mut Outgoing<D> {
        let (prev_state, new_outgoing) = match self.outgoing.entry(addr) {
            Entry::Vacant(vacant) => {
                let inserted = vacant.insert(Outgoing {
                    state: new_state,
                    is_unforgettable: false,
                });

                (None, inserted)
            }

            Entry::Occupied(occupied) => {
                let prev = occupied.into_mut();

                mem::swap(&mut prev.state, &mut new_state);

                // `new_state` and `prev.state` are swapped now.
                (Some(new_state), prev)
            }
        };

        // Update the routing table.
        match (&prev_state, &new_outgoing.state) {
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

        new_outgoing
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
    pub(crate) fn learn_addr(&mut self, dialer: &mut D, addr: SocketAddr, unforgettable: bool) {
        let span = mk_span(addr, self.outgoing.get(&addr));
        span.clone().in_scope(move || {
            match self.outgoing.entry(addr) {
                Entry::Occupied(_) => {
                    debug!("ignoring already known address");
                }
                Entry::Vacant(_vacant) => {
                    info!("connecting to newly learned address");
                    dialer.connect_outgoing(span, addr);
                    let outgoing = self.change_outgoing_state(
                        addr,
                        OutgoingState::Connecting { failures_so_far: 0 },
                    );
                    if outgoing.is_unforgettable != unforgettable {
                        outgoing.is_unforgettable = unforgettable;
                        debug!(unforgettable, "marked");
                    }
                }
            };
        })
    }

    /// Blocks an address.
    ///
    /// Causes any current connection to the address to be terminated and future ones prohibited.
    pub(crate) fn block_addr(&mut self, dialer: &mut D, addr: SocketAddr, now: Instant) {
        let span = mk_span(addr, self.outgoing.get(&addr));

        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Vacant(_vacant) => {
                    info!("address blocked");
                    self.change_outgoing_state(addr, OutgoingState::Blocked { since: now });
                }
                // TODO: Check what happens on close on our end, i.e. can we distinguish in logs
                // between a closed connection on our end vs one that failed?
                Entry::Occupied(occupied) => match occupied.get().state {
                    OutgoingState::Blocked { .. } => {
                        debug!("already blocking address");
                    }
                    OutgoingState::Loopback => {
                        warn!("requested to block ourselves, refusing to do so");
                    }
                    OutgoingState::Connected { ref handle, .. } => {
                        info!("will disconnect peer after it has been blocked");
                        dialer.disconnect_outgoing(span, handle.clone());
                        self.change_outgoing_state(addr, OutgoingState::Blocked { since: now });
                    }
                    _ => {
                        info!("address blocked");
                        self.change_outgoing_state(addr, OutgoingState::Blocked { since: now });
                    }
                },
            });
    }

    /// Removes an address from the block list.
    ///
    /// Does nothing if the address was not blocked.
    pub(crate) fn redeem_addr(&mut self, dialer: &mut D, addr: SocketAddr) {
        let span = mk_span(addr, self.outgoing.get(&addr));
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Vacant(_) => {
                    debug!("ignoring redemption of unknown address");
                }
                Entry::Occupied(occupied) => match occupied.get().state {
                    OutgoingState::Blocked { .. } => {
                        dialer.connect_outgoing(span, addr);
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

    /// Performs housekeeping like reconnection or unblocking peers.
    ///
    /// This function must periodically be called. A good interval is every second.
    fn perform_housekeeping(&mut self, dialer: &mut D, now: Instant) {
        let mut to_forget = Vec::new();
        let mut to_reconnect = Vec::new();

        for (&addr, outgoing) in self.outgoing.iter() {
            let span = mk_span(addr, Some(&outgoing));

            span.in_scope(|| match outgoing.state {
                // Decide whether to attempt reconnecting a failed-waiting address.
                OutgoingState::Waiting {
                    failures_so_far,
                    last_failure,
                    ..
                } => {
                    if failures_so_far > self.config.retry_attempts {
                        if outgoing.is_unforgettable {
                            // Unforgettable addresses simply have their timer reset.
                            info!("resetting unforgettable address");

                            to_reconnect.push((addr, 0));
                        } else {
                            // Address had too many attempts at reconnection, we will forget
                            // it later if forgettable.
                            to_forget.push(addr);

                            info!("gave up on address");
                        }
                    } else {
                        // The address has not exceeded the limit, so check if it is due.
                        let due = last_failure + self.config.calc_backoff(failures_so_far);
                        if now >= due {
                            debug!(attempts = failures_so_far, "attempting reconnection");

                            to_reconnect.push((addr, failures_so_far));
                        }
                    }
                }

                OutgoingState::Blocked { since } => {
                    if now >= since + self.config.unblock_after {
                        info!(
                            seconds_blocked = (now - since).as_secs(),
                            "unblocked address"
                        );

                        to_reconnect.push((addr, 0));
                    }
                }

                _ => {
                    // Entry is ignored. Not outputting any `trace` because this is log spam even at
                    // the `trace` level.
                }
            });
        }

        // Remove all addresses marked for forgetting.
        to_forget.into_iter().for_each(|addr| {
            self.outgoing.remove(&addr);
        });

        // Reconnect all others.
        to_reconnect
            .into_iter()
            .for_each(|(addr, failures_so_far)| {
                let span = mk_span(addr, self.outgoing.get(&addr));
                dialer.connect_outgoing(span.clone(), addr);

                span.in_scope(|| {
                    self.change_outgoing_state(addr, OutgoingState::Connecting { failures_so_far })
                });
            })
    }

    /// Handles the outcome of a dialing attempt.
    ///
    /// Note that reconnects will earliest happen on the next `perform_housekeeping` call.
    pub(crate) fn handle_dial_outcome(&mut self, dialer: &mut D, dial_outcome: DialOutcome<D>) {
        let addr = dial_outcome.addr();
        let span = mk_span(addr, self.outgoing.get(&addr));

        span.clone().in_scope(move || match dial_outcome {
            DialOutcome::Successful {
                addr,
                handle,
                node_id,
                ..
            } => {
                info!("established outgoing connection");

                match self.outgoing.get(&addr) {
                    // If we connected to a blocked address, do not go into connected, but stay
                    // blocked instead.
                    Some(Outgoing{
                        state: OutgoingState::Blocked { .. }, ..
                    }) => {
                        dialer.disconnect_outgoing(span, handle);
                    },
                    _ => {
                        self.change_outgoing_state(
                            addr ,
                            OutgoingState::Connected {
                                peer_id: node_id,
                                handle,
                            },
                        );
                    }
                }
            }
            DialOutcome::Failed { addr, error, when } => {
                info!(err = display_error(&error), "outgoing connection failed");

                if let Some(outgoing) = self.outgoing.get(&addr) {
                    if let OutgoingState::Connecting { failures_so_far } = outgoing.state {
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Waiting {
                                failures_so_far: failures_so_far + 1,
                                error,
                                last_failure: when,
                            },
                        );
                    } else {
                        warn!(
                            "processing dial outcome on a connection that was not marked as connecting"
                        );
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Waiting {
                                failures_so_far: 1,
                                error,
                                last_failure: when,
                            },
                        );
                    }
                } else {
                    warn!("processing dial outcome non-existent connection");
                    self.change_outgoing_state(
                        addr,
                        OutgoingState::Waiting {
                            failures_so_far: 1,
                            error,
                            last_failure: when,
                        },
                    );
                }
            }
            DialOutcome::Loopback { addr } => {
                info!("found loopback address");
                self.change_outgoing_state(addr, OutgoingState::Loopback);
            }
        });
    }

    /// Notifies the connection manager about a dropped connection.
    ///
    /// This will usually result in an immediate reconnection.
    pub(crate) fn handle_connection_drop(&mut self, dialer: &mut D, addr: SocketAddr) {
        let span = mk_span(addr, self.outgoing.get(&addr));

        span.clone().in_scope(move || {
            if let Some(outgoing) = self.outgoing.get(&addr) {
                match outgoing.state {
                    OutgoingState::Waiting { .. }
                    | OutgoingState::Loopback
                    | OutgoingState::Connecting { .. } => {
                        // We should, under normal circumstances, not receive drop notifications for
                        // any of these. Connection failures are handled by the dialer.
                        warn!("unexpected drop notification")
                    }
                    OutgoingState::Connected { .. } => {
                        // Drop the handle, immediately initiate a reconnection.
                        dialer.connect_outgoing(span, addr);
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Connecting { failures_so_far: 0 },
                        );
                    }
                    OutgoingState::Blocked { .. } => {
                        // Blocked addresses ignore connection drops.
                        debug!("received drop notification for blocked connection")
                    }
                }
            } else {
                warn!("received connection drop notification for unknown connection")
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, net::SocketAddr, time::Duration};

    use fake_instant::FakeClock;
    use thiserror::Error;
    use tracing::Span;

    use crate::init_logging;

    use super::{DialOutcome, Dialer, NodeId, OutgoingConfig, OutgoingManager};

    /// Dialer for unit tests.
    ///
    /// Records request for dialing, otherwise does nothing.
    #[derive(Debug, Default)]
    struct TestDialer {
        connection_requests: RefCell<Vec<SocketAddr>>,
        disconnect_requests: RefCell<Vec<u32>>,
    }

    impl TestDialer {
        /// Returns a mutable reference to the recorded connection requests.
        fn connects(&mut self) -> &mut Vec<SocketAddr> {
            self.connection_requests.get_mut()
        }

        /// Returns a mutable reference to the recorded disconnect requests.
        fn disconnects(&mut self) -> &mut Vec<u32> {
            self.disconnect_requests.get_mut()
        }

        /// Clears all recorded requests.
        fn clear(&mut self) {
            self.connects().clear();
            self.disconnects().clear();
        }
    }

    impl Dialer for TestDialer {
        type Handle = u32;

        type Error = TestDialerError;

        fn connect_outgoing(&self, _span: Span, addr: SocketAddr) {
            self.connection_requests
                .try_borrow_mut()
                .unwrap()
                .push(addr);
        }

        fn disconnect_outgoing(&self, _span: Span, handle: Self::Handle) {
            self.disconnect_requests
                .try_borrow_mut()
                .unwrap()
                .push(handle);
        }
    }

    /// Error for test dialer.
    ///
    /// Tracks a configurable id for the error.
    #[derive(Debug, Error)]
    #[error("test dialer error({})", id)]
    struct TestDialerError {
        id: u32,
    }

    /// Setup an outgoing configuration for testing.
    fn test_config() -> OutgoingConfig {
        OutgoingConfig {
            retry_attempts: 3,
            base_timeout: Duration::from_secs(1),
            unblock_after: Duration::from_secs(60),
        }
    }

    #[test]
    fn successful_lifecycle() {
        init_logging();

        let mut rng = crate::new_rng();
        let mut dialer = TestDialer::default();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        let id_a = NodeId::random_tls(&mut rng);

        let mut manager = OutgoingManager::<TestDialer>::new(test_config());

        // We begin by learning a single, regular address.
        manager.learn_addr(&mut dialer, addr_a, false);

        // This should trigger a dial request.
        assert_eq!(dialer.connects(), &vec![addr_a]);

        // Our first connection attempt fails.
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 1 },
                when: FakeClock::now(),
            },
        );

        // The connection should now be in waiting state, but not reconnect, since the minimum delay
        // is 2 second (2*base_timeout).
        dialer.clear();

        // Performing housekeeping multiple times should not make a difference.
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        // Advancing the clock will trigger a reconnection on the next housekeeping.
        FakeClock::advance_time(2_000);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert_eq!(dialer.connects(), &vec![addr_a]);

        // This time the connection succeeds.
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Successful {
                addr: addr_a,
                handle: 99,
                node_id: id_a,
            },
        );

        // The routing table should have been updated and should return the handle.
        assert_eq!(manager.get_route(id_a), Some(&99));

        // Time passes, and our connection drops. Reconnecting should be immediate.
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        FakeClock::advance_time(20_000);
        dialer.clear();
        manager.handle_connection_drop(&mut dialer, addr_a);

        // The route should have been cleared.
        assert!(manager.get_route(id_a).is_none());

        // Reconnecting should be immediate.
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert_eq!(dialer.connects(), &vec![addr_a]);
    }

    #[test]
    fn connections_forgotten_after_too_many_tries() {
        init_logging();

        let mut dialer = TestDialer::default();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        // Address `addr_b` will be a known address.
        let addr_b: SocketAddr = "5.6.7.8:5678".parse().unwrap();

        let mut manager = OutgoingManager::<TestDialer>::new(test_config());

        // First, attempt to connect. Tests are set to 3 retries after 2, 4 and 8 seconds.
        manager.learn_addr(&mut dialer, addr_a, false);
        manager.learn_addr(&mut dialer, addr_b, true);
        assert_eq!(dialer.connects(), &vec![addr_a, addr_b]);

        // Fail the first connection attempts.
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 10 },
                when: FakeClock::now(),
            },
        );
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 11 },
                when: FakeClock::now(),
            },
        );

        // Learning the address again should not cause a reconnection.
        dialer.clear();
        manager.learn_addr(&mut dialer, addr_a, false);
        manager.learn_addr(&mut dialer, addr_b, false);
        assert!(dialer.connects().is_empty());
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        manager.learn_addr(&mut dialer, addr_a, false);
        manager.learn_addr(&mut dialer, addr_b, false);
        assert!(dialer.connects().is_empty());

        // After 1.999 seconds, reconnection should still be delayed.
        FakeClock::advance_time(1_999);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        // Adding 0.001 seconds finally is enough to reconnect.
        FakeClock::advance_time(1);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().contains(&addr_a));
        assert!(dialer.connects().contains(&addr_b));

        // Waiting for more than the reconnection delay should not be harmful or change anything, as
        // we are currently connecting.
        FakeClock::advance_time(6_000);
        dialer.clear();
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        // Fail the connection again, wait 3.999 seconds, expecting no reconnection.
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 40 },
                when: FakeClock::now(),
            },
        );
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 41 },
                when: FakeClock::now(),
            },
        );

        FakeClock::advance_time(3_999);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        // Adding 0.001 seconds finally again pushes us over the threshold.
        FakeClock::advance_time(1);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().contains(&addr_a));
        assert!(dialer.connects().contains(&addr_b));

        // Fail the connection quickly.
        FakeClock::advance_time(25);
        dialer.clear();
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 10 },
                when: FakeClock::now(),
            },
        );
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 10 },
                when: FakeClock::now(),
            },
        );
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        // The last attempt should happen 8 seconds after the error, not the last attempt.
        FakeClock::advance_time(7_999);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());
        FakeClock::advance_time(1);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().contains(&addr_a));
        assert!(dialer.connects().contains(&addr_b));

        // Fail the last attempt. No more reconnections should be happening.
        dialer.clear();
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 10 },
                when: FakeClock::now(),
            },
        );
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 10 },
                when: FakeClock::now(),
            },
        );
        assert!(dialer.connects().is_empty());
        manager.perform_housekeeping(&mut dialer, FakeClock::now());

        // Only the unforgettable address should be reconnecting.
        assert_eq!(dialer.connects(), &vec![addr_b]);

        // But not `addr_a`, even after a long wait.
        dialer.clear();
        FakeClock::advance_time(1_000_000_000);
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dbg!(dialer.connects()).is_empty());
    }

    #[test]
    fn blocking_works() {
        init_logging();

        let mut rng = crate::new_rng();
        let mut dialer = TestDialer::default();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        // We use `addr_b` as an unforgettable address, which does not mean it cannot be blocked!
        let addr_b: SocketAddr = "5.6.7.8:5678".parse().unwrap();
        let addr_c: SocketAddr = "9.0.1.2:9012".parse().unwrap();
        let id_a = NodeId::random_tls(&mut rng);
        let id_b = NodeId::random_tls(&mut rng);
        let id_c = NodeId::random_tls(&mut rng);

        let mut manager = OutgoingManager::<TestDialer>::new(test_config());

        // Block `addr_a` from the start.
        manager.block_addr(&mut dialer, addr_a, FakeClock::now());
        assert!(dialer.connects().is_empty());
        assert!(dialer.disconnects().is_empty());

        // Learning both `addr_a` and `addr_b` should only trigger a connection to `addr_b` now.
        manager.learn_addr(&mut dialer, addr_a, false);
        manager.learn_addr(&mut dialer, addr_b, true);
        assert_eq!(dialer.connects(), &vec![addr_b]);
        dialer.clear();

        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        // Fifteen seconds later we succeed in connecting to `addr_b`.
        FakeClock::advance_time(15_000);
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Successful {
                addr: addr_b,
                handle: 101,
                node_id: id_b,
            },
        );
        assert_eq!(manager.get_route(id_b), Some(&101));

        // Invariant through housekeeping.
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());
        assert_eq!(manager.get_route(id_b), Some(&101));

        // Another fifteen seconds later, we block `addr_b`.
        FakeClock::advance_time(15_000);
        manager.block_addr(&mut dialer, addr_b, FakeClock::now());
        assert!(dialer.connects().is_empty());
        assert_eq!(dialer.disconnects(), &vec![101]);

        // `addr_c` will be blocked during the connection phase.
        dialer.clear();
        manager.learn_addr(&mut dialer, addr_c, false);
        assert_eq!(dialer.connects(), &vec![addr_c]);
        dialer.clear();
        manager.block_addr(&mut dialer, addr_c, FakeClock::now());
        assert!(dialer.connects().is_empty());
        assert!(dialer.disconnects().is_empty());

        // We are still expect to provide a dial outcome, but afterwards, there should be no route
        // to C and an immediate disconnection should be queued.
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Successful {
                addr: addr_c,
                handle: 42,
                node_id: id_c,
            },
        );
        assert_eq!(dialer.disconnects(), &vec![42]);

        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        assert!(manager.get_route(id_c).is_none());

        // At this point, we have blocked all three addresses. 30 seconds later, the first one is
        // unblocked due to the block timing out.
        FakeClock::advance_time(30_000);
        dialer.clear();
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert_eq!(dialer.connects(), &vec![addr_a]);

        // Fifteen seconds later, B and C are still blocked, but we redeem B early.
        FakeClock::advance_time(15_000);
        dialer.clear();
        manager.perform_housekeeping(&mut dialer, FakeClock::now());
        assert!(dialer.connects().is_empty());

        manager.redeem_addr(&mut dialer, addr_b);
        assert_eq!(dialer.connects(), &vec![addr_b]);

        // Succeed both connections, and ensure we have routes to both.
        dialer.clear();
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Successful {
                addr: addr_b,
                handle: 77,
                node_id: id_b,
            },
        );
        manager.handle_dial_outcome(
            &mut dialer,
            DialOutcome::Successful {
                addr: addr_a,
                handle: 66,
                node_id: id_a,
            },
        );
        assert!(dialer.connects().is_empty());
        assert!(dialer.disconnects().is_empty());

        assert_eq!(manager.get_route(id_a), Some(&66));
        assert_eq!(manager.get_route(id_b), Some(&77));
    }

    // TODO: doesn't crash on random input

    // TODO: blocks loopback
}
