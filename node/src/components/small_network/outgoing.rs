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
//! performed through [`DialRequest`] and [`DialOutcome`]s. This frees the `OutgoingManager` from
//! having to worry about protocol specifics.
//!
//! Three conditions not expressed in code must be fulfilled for the `OutgoingManager` to function:
//!
//! * The `Dialer` is expected to produce `DialOutcomes` for every dial [`DialRequest::Dial`]
//!   eventually. These must be forwarded to the `OutgoingManager` via the `handle_dial_outcome`
//!   function.
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
//!                   forget (after n tries)
//!          ┌────────────────────────────────────┐
//!          │                 learn              ▼
//!          │               ┌──────────────  unknown/forgotten
//!          │               │                (implicit state)
//!          │               │
//!          │               │                │
//!          │               │                │ block
//!          │               │                │
//!          │               │                │
//!          │               │                ▼
//!     ┌────┴────┐          │          ┌─────────┐
//!     │         │  fail    │    block │         │
//!     │ Waiting │◄───────┐ │   ┌─────►│ Blocked │◄──────────┐
//! ┌───┤         │        │ │   │      │         │           │
//! │   └────┬────┘        │ │   │      └────┬────┘           │
//! │ block  │             │ │   │           │                │
//! │        │ timeout     │ ▼   │           │ redeem,        │
//! │        │        ┌────┴─────┴───┐       │ block timeout  │
//! │        │        │              │       │                │
//! │        └───────►│  Connecting  │◄──────┘                │
//! │                 │              │                        │
//! │                 └─────┬────┬───┘                        │
//! │                       │ ▲  │                            │
//! │               success │ │  │ detect                     │
//! │                       │ │  │      ┌──────────┐          │
//! │ ┌───────────┐         │ │  │      │          │          │
//! │ │           │◄────────┘ │  │      │ Loopback │          │
//! │ │ Connected │           │  └─────►│          │          │
//! │ │           │ dropped   │         └──────────┘          │
//! │ └─────┬─────┴───────────┘                               │
//! │       │                                                 │
//! │       │ block                                           │
//! └───────┴─────────────────────────────────────────────────┘
//! ```
//!
//! # Timeouts/safety
//!
//! The `sweep` transition for connections usually does not happen during normal operations. Three
//! causes are typical for it:
//!
//! * A configured TCP timeout above [`OutgoingConfig::sweep_timeout`].
//! * Very slow responses from remote peers (similar to a Slowloris-attack)
//! * Faulty handling by the driver of the [`OutgoingManager`], i.e. the outside component.
//!
//! Should a dial attempt exceed a certain timeout, it is considered failed and put into the waiting
//! state again.
//!
//! If a conflict (multiple successful dial results) occurs, the more recent connection takes
//! precedence over the previous one. This prevents problems when a notification of a terminated
//! connection is overtaken by the new connection announcement.

// Clippy has a lot of false positives due to `span.clone()`-closures.
#![allow(clippy::redundant_clone)]

use std::{
    collections::{hash_map::Entry, HashMap},
    error::Error,
    fmt::{self, Debug, Display, Formatter},
    mem,
    net::SocketAddr,
    time::{Duration, Instant},
};

use datasize::DataSize;

use prometheus::IntGauge;
use tracing::{debug, error_span, field::Empty, info, trace, warn, Span};

use super::{display_error, NodeId};

/// An outgoing connection/address in various states.
#[derive(DataSize, Debug)]
pub struct Outgoing<H, E>
where
    H: DataSize,
    E: DataSize,
{
    /// Whether or not the address is unforgettable, see `learn_addr` for details.
    is_unforgettable: bool,
    /// The current state the connection/address is in.
    state: OutgoingState<H, E>,
}

/// Active state for a connection/address.
#[derive(DataSize, Debug)]
pub enum OutgoingState<H, E>
where
    H: DataSize,
    E: DataSize,
{
    /// The outgoing address has been known for the first time and we are currently connecting.
    Connecting {
        /// Number of attempts that failed, so far.
        failures_so_far: u8,
        /// Time when the connection attempt was instantiated.
        since: Instant,
    },
    /// The connection has failed at least one connection attempt and is waiting for a retry.
    Waiting {
        /// Number of attempts that failed, so far.
        failures_so_far: u8,
        /// The most recent connection error.
        ///
        /// If not given, the connection was put into a `Waiting` state due to a sweep timeout.
        error: Option<E>,
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
        handle: H,
    },
    /// The address was blocked and will not be retried.
    Blocked { since: Instant },
    /// The address is owned by ourselves and will not be tried again.
    Loopback,
}

impl<H, E> Display for OutgoingState<H, E>
where
    H: DataSize,
    E: DataSize,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OutgoingState::Connecting {
                failures_so_far, ..
            } => {
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
pub enum DialOutcome<H, E> {
    /// A connection was successfully established.
    Successful {
        /// The address dialed.
        addr: SocketAddr,
        /// A handle to send data down the connection.
        handle: H,
        /// The remote peer's authenticated node ID.
        node_id: NodeId,
    },
    /// The connection attempt failed.
    Failed {
        /// The address dialed.
        addr: SocketAddr,
        /// The error encountered while dialing.
        error: E,
        /// The moment the connection attempt failed.
        when: Instant,
    },
    /// The connection was aborted, because the remote peer turned out to be a loopback.
    Loopback {
        /// The address used to connect.
        addr: SocketAddr,
    },
}

impl<H, E> DialOutcome<H, E> {
    /// Retrieves the socket address from the `DialOutcome`.
    fn addr(&self) -> SocketAddr {
        match self {
            DialOutcome::Successful { addr, .. } => *addr,
            DialOutcome::Failed { addr, .. } => *addr,
            DialOutcome::Loopback { addr, .. } => *addr,
        }
    }
}

/// A request made for dialing.
#[derive(Clone, Debug)]
#[must_use]
pub(crate) enum DialRequest<H> {
    /// Attempt to connect to the outgoing socket address.
    ///
    /// For every time this request is emitted, there must be a corresponding call to
    /// `handle_dial_outcome` eventually.
    ///
    /// Any logging of connection issues should be done in the context of `span` for better log
    /// output.
    Dial { addr: SocketAddr, span: Span },

    /// Disconnects a potentially existing connection.
    ///
    /// Used when a peer has been blocked or should be disconnected for other reasons.
    Disconnect { handle: H, span: Span },
}

impl<H> Display for DialRequest<H>
where
    H: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DialRequest::Dial { addr, .. } => {
                write!(f, "dial: {}", addr)
            }
            DialRequest::Disconnect { handle, .. } => {
                write!(f, "disconnect: {}", handle)
            }
        }
    }
}

#[derive(DataSize, Debug)]
/// Connection settings for the outgoing connection manager.
pub struct OutgoingConfig {
    /// The maximum number of attempts before giving up and forgetting an address, if permitted.
    pub(crate) retry_attempts: u8,
    /// The basic time slot for exponential backoff when reconnecting.
    pub(crate) base_timeout: Duration,
    /// Time until an outgoing address is unblocked.
    pub(crate) unblock_after: Duration,
    /// Safety timeout, after which a connection is no longer expected to finish dialing.
    pub(crate) sweep_timeout: Duration,
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
#[derive(DataSize, Debug)]
pub struct OutgoingManager<H, E>
where
    H: DataSize,
    E: DataSize,
{
    /// Outgoing connections subsystem configuration.
    config: OutgoingConfig,
    /// Mapping of address to their current connection state.
    outgoing: HashMap<SocketAddr, Outgoing<H, E>>,
    /// Routing table.
    ///
    /// Contains a mapping from node IDs to connected socket addresses. A missing entry means that
    /// the destination is not connected.
    routes: HashMap<NodeId, SocketAddr>,
    /// A set of outgoing metrics.
    #[data_size(skip)]
    metrics: OutgoingMetrics,
}

/// A set of metrics used by the outgoing component.
#[derive(Clone, Debug)]
pub(super) struct OutgoingMetrics {
    /// Number of outgoing connections in connecting state.
    pub(super) out_state_connecting: IntGauge,
    /// Number of outgoing connections in waiting state.
    pub(super) out_state_waiting: IntGauge,
    /// Number of outgoing connections in connected state.
    pub(super) out_state_connected: IntGauge,
    /// Number of outgoing connections in blocked state.
    pub(super) out_state_blocked: IntGauge,
    /// Number of outgoing connections in loopback state.
    pub(super) out_state_loopback: IntGauge,
}

// Note: We only implement `Default` here for use in testing with `OutgoingManager::new`.
#[cfg(test)]
impl Default for OutgoingMetrics {
    fn default() -> Self {
        Self {
            out_state_connecting: IntGauge::new(
                "out_state_connecting",
                "internal out_state_connecting",
            )
            .unwrap(),
            out_state_waiting: IntGauge::new("out_state_waiting", "internal out_state_waiting")
                .unwrap(),
            out_state_connected: IntGauge::new(
                "out_state_connected",
                "internal out_state_connected",
            )
            .unwrap(),
            out_state_blocked: IntGauge::new("out_state_blocked", "internal out_state_blocked")
                .unwrap(),
            out_state_loopback: IntGauge::new("out_state_loopback", "internal loopback").unwrap(),
        }
    }
}

impl<H, E> OutgoingManager<H, E>
where
    H: DataSize,
    E: DataSize,
{
    /// Creates a new outgoing manager with a set of metrics that is not connected to any registry.
    #[cfg(test)]
    #[inline]
    pub(super) fn new(config: OutgoingConfig) -> Self {
        Self::with_metrics(config, Default::default())
    }

    /// Creates a new outgoing manager with an already existing set of metrics.
    pub(super) fn with_metrics(config: OutgoingConfig, metrics: OutgoingMetrics) -> Self {
        Self {
            config,
            outgoing: Default::default(),
            routes: Default::default(),
            metrics,
        }
    }

    /// Returns a reference to the internal metrics.
    #[cfg(test)]
    fn metrics(&self) -> &OutgoingMetrics {
        &self.metrics
    }
}

/// Creates a logging span for a specific connection.
#[inline]
fn make_span<H, E>(addr: SocketAddr, outgoing: Option<&Outgoing<H, E>>) -> Span
where
    H: DataSize,
    E: DataSize,
{
    // Note: The jury is still out on whether we want to create a single span per connection and
    // cache it, or create a new one (with the same connection ID) each time this is called. The
    // advantage of the former is external tools have it easier correlating all related
    // information, while the drawback is not being able to change the parent span link, which
    // might be awkward.

    if let Some(outgoing) = outgoing {
        match outgoing.state {
            OutgoingState::Connected { peer_id, .. } => {
                error_span!("outgoing", %addr, state=%outgoing.state, %peer_id, validator_id=Empty)
            }
            _ => {
                error_span!("outgoing", %addr, state=%outgoing.state, peer_id=Empty, validator_id=Empty)
            }
        }
    } else {
        error_span!("outgoing", %addr, state = "-")
    }
}

impl<H, E> OutgoingManager<H, E>
where
    H: DataSize + Clone,
    E: DataSize + Error,
{
    /// Changes the state of an outgoing connection.
    ///
    /// Will trigger an update of the routing table if necessary. Does not emit any other
    /// side-effects.
    fn change_outgoing_state(
        &mut self,
        addr: SocketAddr,
        mut new_state: OutgoingState<H, E>,
    ) -> &mut Outgoing<H, E> {
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
                trace!("route unchanged, already connected");
            }

            // Dropping from connected to any other state requires clearing the route.
            (Some(OutgoingState::Connected { peer_id, .. }), _) => {
                debug!(%peer_id, "route removed");
                self.routes.remove(peer_id);
            }

            // Otherwise we have established a new route.
            (_, OutgoingState::Connected { peer_id, .. }) => {
                debug!(%peer_id, "route added");
                self.routes.insert(*peer_id, addr);
            }

            _ => {
                trace!("route unchanged");
            }
        }

        // Update the metrics, decreasing the count of the state that was left, while increasing
        // the new state. Note that this will lead to a non-atomic dec/inc if the previous state
        // was the same as before.
        match prev_state {
            Some(OutgoingState::Blocked { .. }) => self.metrics.out_state_blocked.dec(),
            Some(OutgoingState::Connected { .. }) => self.metrics.out_state_connected.dec(),
            Some(OutgoingState::Connecting { .. }) => self.metrics.out_state_connecting.dec(),
            Some(OutgoingState::Loopback) => self.metrics.out_state_loopback.dec(),
            Some(OutgoingState::Waiting { .. }) => self.metrics.out_state_waiting.dec(),
            None => {
                // Nothing to do, there was no previous state.
            }
        }

        match new_outgoing.state {
            OutgoingState::Blocked { .. } => self.metrics.out_state_blocked.inc(),
            OutgoingState::Connected { .. } => self.metrics.out_state_connected.inc(),
            OutgoingState::Connecting { .. } => self.metrics.out_state_connecting.inc(),
            OutgoingState::Loopback => self.metrics.out_state_loopback.inc(),
            OutgoingState::Waiting { .. } => self.metrics.out_state_waiting.inc(),
        }

        new_outgoing
    }

    /// Retrieves the address by peer.
    pub(crate) fn get_addr(&self, peer_id: NodeId) -> Option<SocketAddr> {
        self.routes.get(&peer_id).copied()
    }

    /// Retrieves a handle to a peer.
    ///
    /// Primary function to send data to peers; clients retrieve a handle to it which can then
    /// be used to send data.
    pub(crate) fn get_route(&self, peer_id: NodeId) -> Option<&H> {
        let outgoing = self.outgoing.get(self.routes.get(&peer_id)?)?;

        if let OutgoingState::Connected { ref handle, .. } = outgoing.state {
            Some(handle)
        } else {
            None
        }
    }

    /// Iterates over all connected peer IDs.
    pub(crate) fn connected_peers(&'_ self) -> impl Iterator<Item = NodeId> + '_ {
        self.routes.keys().cloned()
    }

    /// Notify about a potentially new address that has been discovered.
    ///
    /// Immediately triggers the connection process to said address if it was not known before.
    ///
    /// A connection marked `unforgettable` will never be evicted but reset instead when it exceeds
    /// the retry limit.
    pub(crate) fn learn_addr(
        &mut self,
        addr: SocketAddr,
        unforgettable: bool,
        now: Instant,
    ) -> Option<DialRequest<H>> {
        let span = make_span(addr, self.outgoing.get(&addr));
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Occupied(_) => {
                    debug!("ignoring already known address");
                    None
                }
                Entry::Vacant(_vacant) => {
                    info!("connecting to newly learned address");
                    let outgoing = self.change_outgoing_state(
                        addr,
                        OutgoingState::Connecting {
                            failures_so_far: 0,
                            since: now,
                        },
                    );
                    if outgoing.is_unforgettable != unforgettable {
                        outgoing.is_unforgettable = unforgettable;
                        debug!(unforgettable, "marked");
                    }
                    Some(DialRequest::Dial { addr, span })
                }
            })
    }

    /// Blocks an address.
    ///
    /// Causes any current connection to the address to be terminated and future ones prohibited.
    pub(crate) fn block_addr(&mut self, addr: SocketAddr, now: Instant) -> Option<DialRequest<H>> {
        let span = make_span(addr, self.outgoing.get(&addr));

        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Vacant(_vacant) => {
                    info!("unknown address blocked");
                    self.change_outgoing_state(addr, OutgoingState::Blocked { since: now });
                    None
                }
                // TODO: Check what happens on close on our end, i.e. can we distinguish in logs
                // between a closed connection on our end vs one that failed?
                Entry::Occupied(occupied) => match occupied.get().state {
                    OutgoingState::Blocked { .. } => {
                        debug!("address already blocked");
                        None
                    }
                    OutgoingState::Loopback => {
                        warn!("loopback address block ignored");
                        None
                    }
                    OutgoingState::Connected { ref handle, .. } => {
                        info!("connected address blocked, disconnecting");
                        let handle = handle.clone();
                        self.change_outgoing_state(addr, OutgoingState::Blocked { since: now });
                        Some(DialRequest::Disconnect { span, handle })
                    }
                    OutgoingState::Waiting { .. } | OutgoingState::Connecting { .. } => {
                        info!("address blocked");
                        self.change_outgoing_state(addr, OutgoingState::Blocked { since: now });
                        None
                    }
                },
            })
    }

    /// Checks if an address is blocked.
    #[cfg(test)]
    pub(crate) fn is_blocked(&self, addr: SocketAddr) -> bool {
        match self.outgoing.get(&addr) {
            Some(outgoing) => matches!(outgoing.state, OutgoingState::Blocked { .. }),
            None => false,
        }
    }

    /// Removes an address from the block list.
    ///
    /// Does nothing if the address was not blocked.
    // This function is currently not in use by `small_network` itself.
    #[allow(dead_code)]
    pub(crate) fn redeem_addr(&mut self, addr: SocketAddr, now: Instant) -> Option<DialRequest<H>> {
        let span = make_span(addr, self.outgoing.get(&addr));
        span.clone()
            .in_scope(move || match self.outgoing.entry(addr) {
                Entry::Vacant(_) => {
                    debug!("unknown address redeemed");
                    None
                }
                Entry::Occupied(occupied) => match occupied.get().state {
                    OutgoingState::Blocked { .. } => {
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Connecting {
                                failures_so_far: 0,
                                since: now,
                            },
                        );
                        Some(DialRequest::Dial { addr, span })
                    }
                    _ => {
                        debug!("address redemption ignored, not blocked");
                        None
                    }
                },
            })
    }

    /// Performs housekeeping like reconnection or unblocking peers.
    ///
    /// This function must periodically be called. A good interval is every second.
    pub(super) fn perform_housekeeping(&mut self, now: Instant) -> Vec<DialRequest<H>> {
        let mut to_forget = Vec::new();
        let mut to_fail = Vec::new();
        let mut to_reconnect = Vec::new();

        for (&addr, outgoing) in self.outgoing.iter() {
            let span = make_span(addr, Some(outgoing));

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
                            info!("unforgettable address reset");

                            to_reconnect.push((addr, 0));
                        } else {
                            // Address had too many attempts at reconnection, we will forget
                            // it after exiting this closure.
                            to_forget.push(addr);

                            info!("address forgotten");
                        }
                    } else {
                        // The address has not exceeded the limit, so check if it is due.
                        let due = last_failure + self.config.calc_backoff(failures_so_far);
                        if now >= due {
                            debug!(attempts = failures_so_far, "address reconnecting");

                            to_reconnect.push((addr, failures_so_far));
                        }
                    }
                }

                OutgoingState::Blocked { since } => {
                    if now >= since + self.config.unblock_after {
                        info!("address unblocked");

                        to_reconnect.push((addr, 0));
                    }
                }

                OutgoingState::Connecting {
                    since,
                    failures_so_far,
                } => {
                    let timeout = since + self.config.sweep_timeout;
                    if now >= timeout {
                        // The outer component has not called us with a `DialOutcome` in a
                        // reasonable amount of time. This should happen very rarely, ideally
                        // never.
                        warn!("address timed out connecting, was swept");

                        // Count the timeout as a failure against the connection.
                        to_fail.push((addr, failures_so_far + 1));
                    }
                }

                OutgoingState::Connected { .. } | OutgoingState::Loopback => {
                    // Entry is ignored. Not outputting any `trace` because this is log spam even at
                    // the `trace` level.
                }
            });
        }

        // Remove all addresses marked for forgetting.
        to_forget.into_iter().for_each(|addr| {
            self.outgoing.remove(&addr);
        });

        // Fail connections that are taking way too long to connect.
        to_fail.into_iter().for_each(|(addr, failures_so_far)| {
            let span = make_span(addr, self.outgoing.get(&addr));

            span.in_scope(|| {
                self.change_outgoing_state(
                    addr,
                    OutgoingState::Waiting {
                        failures_so_far,
                        error: None,
                        last_failure: now,
                    },
                )
            });
        });

        // Reconnect all others.
        to_reconnect
            .into_iter()
            .map(|(addr, failures_so_far)| {
                let span = make_span(addr, self.outgoing.get(&addr));

                span.clone().in_scope(|| {
                    self.change_outgoing_state(
                        addr,
                        OutgoingState::Connecting {
                            failures_so_far,
                            since: now,
                        },
                    )
                });

                DialRequest::Dial { addr, span }
            })
            .collect()
    }

    /// Handles the outcome of a dialing attempt.
    ///
    /// Note that reconnects will earliest happen on the next `perform_housekeeping` call.
    pub(crate) fn handle_dial_outcome(
        &mut self,
        dial_outcome: DialOutcome<H, E>,
    ) -> Option<DialRequest<H>> {
        let addr = dial_outcome.addr();
        let span = make_span(addr, self.outgoing.get(&addr));

        span.clone().in_scope(move || match dial_outcome {
            DialOutcome::Successful {
                addr,
                handle,
                node_id,
                ..
            } => {
                info!("established outgoing connection");

                if let Some(Outgoing{
                    state: OutgoingState::Blocked { .. }, ..
                }) = self.outgoing.get(&addr) {
                    // If we connected to a blocked address, do not go into connected, but stay
                    // blocked instead.
                    Some(DialRequest::Disconnect{
                        handle, span
                    })
                } else {
                    // Otherwise, just record the connected state.
                    self.change_outgoing_state(
                        addr,
                        OutgoingState::Connected {
                            peer_id: node_id,
                            handle,
                        },
                    );
                    None
                }
            }

            DialOutcome::Failed { addr, error, when } => {
                info!(err = display_error(&error), "outgoing connection failed");

                if let Some(outgoing) = self.outgoing.get(&addr) {
                    match outgoing.state {
                        OutgoingState::Connecting { failures_so_far,.. } => {
                            self.change_outgoing_state(
                                addr,
                                OutgoingState::Waiting {
                                    failures_so_far: failures_so_far + 1,
                                    error: Some(error),
                                    last_failure: when,
                                },
                            );
                            None
                        }
                        OutgoingState::Blocked { .. } => {
                            debug!("failed dial outcome after block ignored");

                            // We do not set the connection to "waiting" if an out-of-order failed
                            // connection arrives, but continue to honor the blocking.
                            None
                        }
                        OutgoingState::Waiting { .. } |
                        OutgoingState::Connected { .. } |
                        OutgoingState::Loopback => {
                            warn!(
                                "processing dial outcome on a connection that was not marked as connecting or blocked"
                            );

                            None
                        }
                    }
                } else {
                    warn!("processing dial outcome non-existent connection");

                    // If the connection does not exist, do not introduce it!
                    None
                }
            }
            DialOutcome::Loopback { addr } => {
                info!("found loopback address");
                self.change_outgoing_state(addr, OutgoingState::Loopback);
                None
            }
        })
    }

    /// Notifies the connection manager about a dropped connection.
    ///
    /// This will usually result in an immediate reconnection.
    pub(crate) fn handle_connection_drop(
        &mut self,
        addr: SocketAddr,
        now: Instant,
    ) -> Option<DialRequest<H>> {
        let span = make_span(addr, self.outgoing.get(&addr));

        span.clone().in_scope(move || {
            if let Some(outgoing) = self.outgoing.get(&addr) {
                match outgoing.state {
                    OutgoingState::Waiting { .. }
                    | OutgoingState::Loopback
                    | OutgoingState::Connecting { .. } => {
                        // We should, under normal circumstances, not receive drop notifications for
                        // any of these. Connection failures are handled by the dialer.
                        warn!("unexpected drop notification");
                        None
                    }
                    OutgoingState::Connected { .. } => {
                        // Drop the handle, immediately initiate a reconnection.
                        self.change_outgoing_state(
                            addr,
                            OutgoingState::Connecting {
                                failures_so_far: 0,
                                since: now,
                            },
                        );
                        Some(DialRequest::Dial { addr, span })
                    }
                    OutgoingState::Blocked { .. } => {
                        // Blocked addresses ignore connection drops.
                        debug!("received drop notification for blocked connection");
                        None
                    }
                }
            } else {
                warn!("received connection drop notification for unknown connection");
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use datasize::DataSize;
    use thiserror::Error;

    use super::{DialOutcome, DialRequest, NodeId, OutgoingConfig, OutgoingManager};
    use crate::testing::{init_logging, test_clock::TestClock};

    /// Error for test dialer.
    ///
    /// Tracks a configurable id for the error.
    #[derive(DataSize, Debug, Error)]
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
            sweep_timeout: Duration::from_secs(45),
        }
    }

    /// Helper function that checks if a given dial request actually dials the expected address.
    fn dials<'a, H, T>(expected: SocketAddr, requests: T) -> bool
    where
        T: IntoIterator<Item = &'a DialRequest<H>> + 'a,
        H: 'a,
    {
        for req in requests.into_iter() {
            if let DialRequest::Dial { addr, .. } = req {
                if *addr == expected {
                    return true;
                }
            }
        }

        false
    }

    /// Helper function that checks if a given dial request actually disconnects the expected
    /// address.
    fn disconnects<'a, H, T>(expected: H, requests: T) -> bool
    where
        T: IntoIterator<Item = &'a DialRequest<H>> + 'a,
        H: 'a + PartialEq,
    {
        for req in requests.into_iter() {
            if let DialRequest::Disconnect { handle, .. } = req {
                if *handle == expected {
                    return true;
                }
            }
        }

        false
    }

    #[test]
    fn successful_lifecycle() {
        init_logging();

        let mut rng = crate::new_rng();
        let mut clock = TestClock::new();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        let id_a = NodeId::random(&mut rng);

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        // We begin by learning a single, regular address, triggering a dial request.
        assert!(dials(
            addr_a,
            &manager.learn_addr(addr_a, false, clock.now())
        ));
        assert_eq!(manager.metrics().out_state_connecting.get(), 1);

        // Our first connection attempt fails. The connection should now be in waiting state, but
        // not reconnect, since the minimum delay is 2 seconds (2*base_timeout).
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 1 },
                when: clock.now(),
            },)
            .is_none());
        assert_eq!(manager.metrics().out_state_connecting.get(), 0);
        assert_eq!(manager.metrics().out_state_waiting.get(), 1);

        // Performing housekeeping multiple times should not make a difference.
        assert!(manager.perform_housekeeping(clock.now()).is_empty());
        assert!(manager.perform_housekeeping(clock.now()).is_empty());
        assert!(manager.perform_housekeeping(clock.now()).is_empty());
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // Advancing the clock will trigger a reconnection on the next housekeeping.
        clock.advance_time(2_000);
        assert!(dials(addr_a, &manager.perform_housekeeping(clock.now())));
        assert_eq!(manager.metrics().out_state_connecting.get(), 1);
        assert_eq!(manager.metrics().out_state_waiting.get(), 0);

        // This time the connection succeeds.
        assert!(manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: addr_a,
                handle: 99,
                node_id: id_a,
            },)
            .is_none());
        assert_eq!(manager.metrics().out_state_connecting.get(), 0);
        assert_eq!(manager.metrics().out_state_connected.get(), 1);

        // The routing table should have been updated and should return the handle.
        assert_eq!(manager.get_route(id_a), Some(&99));
        assert_eq!(manager.get_addr(id_a), Some(addr_a));

        // Time passes, and our connection drops. Reconnecting should be immediate.
        assert!(manager.perform_housekeeping(clock.now()).is_empty());
        clock.advance_time(20_000);
        assert!(dials(
            addr_a,
            &manager.handle_connection_drop(addr_a, clock.now())
        ));
        assert_eq!(manager.metrics().out_state_connecting.get(), 1);
        assert_eq!(manager.metrics().out_state_waiting.get(), 0);

        // The route should have been cleared.
        assert!(manager.get_route(id_a).is_none());
        assert!(manager.get_addr(id_a).is_none());

        // Reconnection is already in progress, so we do not expect another request on housekeeping.
        assert!(manager.perform_housekeeping(clock.now()).is_empty());
    }

    #[test]
    fn connections_forgotten_after_too_many_tries() {
        init_logging();

        let mut clock = TestClock::new();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        // Address `addr_b` will be a known address.
        let addr_b: SocketAddr = "5.6.7.8:5678".parse().unwrap();

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        // First, attempt to connect. Tests are set to 3 retries after 2, 4 and 8 seconds.
        assert!(dials(
            addr_a,
            &manager.learn_addr(addr_a, false, clock.now())
        ));
        assert!(dials(
            addr_b,
            &manager.learn_addr(addr_b, true, clock.now())
        ));

        // Fail the first connection attempts, not triggering a retry (timeout not reached yet).
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 10 },
                when: clock.now(),
            },)
            .is_none());
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 11 },
                when: clock.now(),
            },)
            .is_none());

        // Learning the address again should not cause a reconnection.
        assert!(manager.learn_addr(addr_a, false, clock.now()).is_none());
        assert!(manager.learn_addr(addr_b, false, clock.now()).is_none());

        assert!(manager.perform_housekeeping(clock.now()).is_empty());
        assert!(manager.learn_addr(addr_a, false, clock.now()).is_none());
        assert!(manager.learn_addr(addr_b, false, clock.now()).is_none());

        // After 1.999 seconds, reconnection should still be delayed.
        clock.advance_time(1_999);
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // Adding 0.001 seconds finally is enough to reconnect.
        clock.advance_time(1);
        let requests = manager.perform_housekeeping(clock.now());
        assert!(dials(addr_a, &requests));
        assert!(dials(addr_b, &requests));

        // Waiting for more than the reconnection delay should not be harmful or change
        // anything, as  we are currently connecting.
        clock.advance_time(6_000);

        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // Fail the connection again, wait 3.999 seconds, expecting no reconnection.
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 40 },
                when: clock.now(),
            },)
            .is_none());
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 41 },
                when: clock.now(),
            },)
            .is_none());

        clock.advance_time(3_999);
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // Adding 0.001 seconds finally again pushes us over the threshold.
        clock.advance_time(1);
        let requests = manager.perform_housekeeping(clock.now());
        assert!(dials(addr_a, &requests));
        assert!(dials(addr_b, &requests));

        // Fail the connection quickly.
        clock.advance_time(25);
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 10 },
                when: clock.now(),
            },)
            .is_none());
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 10 },
                when: clock.now(),
            },)
            .is_none());
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // The last attempt should happen 8 seconds after the error, not the last attempt.
        clock.advance_time(7_999);
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        clock.advance_time(1);
        let requests = manager.perform_housekeeping(clock.now());
        assert!(dials(addr_a, &requests));
        assert!(dials(addr_b, &requests));

        // Fail the last attempt. No more reconnections should be happening.
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 10 },
                when: clock.now(),
            },)
            .is_none());
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_b,
                error: TestDialerError { id: 10 },
                when: clock.now(),
            },)
            .is_none());

        // Only the unforgettable address should be reconnecting.
        let requests = manager.perform_housekeeping(clock.now());
        assert!(!dials(addr_a, &requests));
        assert!(dials(addr_b, &requests));

        // But not `addr_a`, even after a long wait.
        clock.advance_time(1_000_000_000);
        assert!(manager.perform_housekeeping(clock.now()).is_empty());
    }

    #[test]
    fn blocking_works() {
        init_logging();

        let mut rng = crate::new_rng();
        let mut clock = TestClock::new();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        // We use `addr_b` as an unforgettable address, which does not mean it cannot be blocked!
        let addr_b: SocketAddr = "5.6.7.8:5678".parse().unwrap();
        let addr_c: SocketAddr = "9.0.1.2:9012".parse().unwrap();
        let id_a = NodeId::random(&mut rng);
        let id_b = NodeId::random(&mut rng);
        let id_c = NodeId::random(&mut rng);

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        // Block `addr_a` from the start.
        assert!(manager.block_addr(addr_a, clock.now()).is_none());

        // Learning both `addr_a` and `addr_b` should only trigger a connection to `addr_b` now.
        assert!(manager.learn_addr(addr_a, false, clock.now()).is_none());
        assert!(dials(
            addr_b,
            &manager.learn_addr(addr_b, true, clock.now())
        ));

        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // Fifteen seconds later we succeed in connecting to `addr_b`.
        clock.advance_time(15_000);
        assert!(manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: addr_b,
                handle: 101,
                node_id: id_b,
            },)
            .is_none());
        assert_eq!(manager.get_route(id_b), Some(&101));

        // Invariant through housekeeping.
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        assert_eq!(manager.get_route(id_b), Some(&101));

        // Another fifteen seconds later, we block `addr_b`.
        clock.advance_time(15_000);
        assert!(disconnects(101, &manager.block_addr(addr_b, clock.now())));

        // `addr_c` will be blocked during the connection phase.
        assert!(dials(
            addr_c,
            &manager.learn_addr(addr_c, false, clock.now())
        ));
        assert!(manager.block_addr(addr_c, clock.now()).is_none());

        // We are still expect to provide a dial outcome, but afterwards, there should be no
        // route to C and an immediate disconnection should be queued.
        assert!(disconnects(
            42,
            &manager.handle_dial_outcome(DialOutcome::Successful {
                addr: addr_c,
                handle: 42,
                node_id: id_c,
            },)
        ));

        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        assert!(manager.get_route(id_c).is_none());

        // At this point, we have blocked all three addresses. 30 seconds later, the first one is
        // unblocked due to the block timing out.

        clock.advance_time(30_000);
        assert!(dials(addr_a, &manager.perform_housekeeping(clock.now())));

        // Fifteen seconds later, B and C are still blocked, but we redeem B early.
        clock.advance_time(15_000);
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        assert!(dials(addr_b, &manager.redeem_addr(addr_b, clock.now())));

        // Succeed both connections, and ensure we have routes to both.
        assert!(manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: addr_b,
                handle: 77,
                node_id: id_b,
            },)
            .is_none());
        assert!(manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: addr_a,
                handle: 66,
                node_id: id_a,
            },)
            .is_none());

        assert_eq!(manager.get_route(id_a), Some(&66));
        assert_eq!(manager.get_route(id_b), Some(&77));
    }

    #[test]
    fn loopback_handled_correctly() {
        init_logging();

        let mut clock = TestClock::new();

        let loopback_addr: SocketAddr = "1.2.3.4:1234".parse().unwrap();

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        // Loopback addresses are connected to only once, and then marked as loopback forever.
        assert!(dials(
            loopback_addr,
            &manager.learn_addr(loopback_addr, false, clock.now())
        ));

        assert!(manager
            .handle_dial_outcome(DialOutcome::Loopback {
                addr: loopback_addr,
            },)
            .is_none());

        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // Learning loopbacks again should not trigger another connection
        assert!(manager
            .learn_addr(loopback_addr, false, clock.now())
            .is_none());

        // Blocking loopbacks does not result in a block, since regular blocks would clear after
        // some time.
        assert!(manager.block_addr(loopback_addr, clock.now()).is_none());

        clock.advance_time(1_000_000_000);

        assert!(manager.perform_housekeeping(clock.now()).is_empty());
    }

    #[test]
    fn connected_peers_works() {
        init_logging();

        let mut rng = crate::new_rng();
        let clock = TestClock::new();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        let addr_b: SocketAddr = "5.6.7.8:5678".parse().unwrap();

        let id_a = NodeId::random(&mut rng);
        let id_b = NodeId::random(&mut rng);

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        manager.learn_addr(addr_a, false, clock.now());
        manager.learn_addr(addr_b, true, clock.now());

        manager.handle_dial_outcome(DialOutcome::Successful {
            addr: addr_a,
            handle: 22,
            node_id: id_a,
        });
        manager.handle_dial_outcome(DialOutcome::Successful {
            addr: addr_b,
            handle: 33,
            node_id: id_b,
        });

        let mut peer_ids: Vec<_> = manager.connected_peers().collect();
        let mut expected = vec![id_a, id_b];

        peer_ids.sort();
        expected.sort();

        assert_eq!(peer_ids, expected);
    }

    #[test]
    fn sweeping_works() {
        init_logging();

        let mut rng = crate::new_rng();
        let mut clock = TestClock::new();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();

        let id_a = NodeId::random(&mut rng);

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        // Trigger a new connection via learning an address.
        assert!(dials(
            addr_a,
            &manager.learn_addr(addr_a, false, clock.now())
        ));

        // We now let enough time pass to cause the connection to be considered failed aborted.
        // No effects are expected at this point.
        clock.advance_time(50_000);
        assert!(manager.perform_housekeeping(clock.now()).is_empty());

        // The connection will now experience a regular failure. Since this is the first connection
        // failure, it should reconnect after 2 seconds.
        clock.advance_time(2_000);
        assert!(dials(addr_a, &manager.perform_housekeeping(clock.now())));

        // We now simulate the second connection (`handle: 2`) succeeding first, after 1 second.
        clock.advance_time(1_000);
        assert!(manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: addr_a,
                handle: 2,
                node_id: id_a,
            })
            .is_none());

        // A route should now be established.
        assert_eq!(manager.get_route(id_a), Some(&2));

        // More time passes and the first connection attempt finally finishes.
        clock.advance_time(30_000);
        assert!(manager
            .handle_dial_outcome(DialOutcome::Successful {
                addr: addr_a,
                handle: 1,
                node_id: id_a,
            })
            .is_none());

        // We now expect to be connected through the first connection (see documentation).
        assert_eq!(manager.get_route(id_a), Some(&1));
    }

    #[test]
    fn blocking_not_overridden_by_racing_failed_connections() {
        init_logging();

        let mut clock = TestClock::new();

        let addr_a: SocketAddr = "1.2.3.4:1234".parse().unwrap();

        let mut manager = OutgoingManager::<u32, TestDialerError>::new(test_config());

        assert!(!manager.is_blocked(addr_a));

        // Block `addr_a` from the start.
        assert!(manager.block_addr(addr_a, clock.now()).is_none());
        assert!(manager.is_blocked(addr_a));

        clock.advance_time(60);

        // Receive an "illegal" dial outcome, even though we did not dial.
        assert!(manager
            .handle_dial_outcome(DialOutcome::Failed {
                addr: addr_a,
                error: TestDialerError { id: 12345 },

                /// The moment the connection attempt failed.
                when: clock.now(),
            })
            .is_none());

        // The failed connection should _not_ have reset the block!
        assert!(manager.is_blocked(addr_a));
        clock.advance_time(60);
        assert!(manager.is_blocked(addr_a));

        assert!(manager.perform_housekeeping(clock.now()).is_empty());
        assert!(manager.is_blocked(addr_a));
    }
}
