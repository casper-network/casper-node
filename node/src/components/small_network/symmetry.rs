//! Connection symmetry management.
//!
//! Tracks the state of connections, which may be uni- or bi-directional, depending on whether a
//! peer has connected back to us. Asymmetric connections are usually removed periodically.

use std::{
    collections::BTreeSet,
    mem,
    net::SocketAddr,
    time::{Duration, Instant},
};

use datasize::DataSize;
use tracing::{debug, warn};

/// Describes whether a connection is uni- or bi-directional.
#[derive(DataSize, Debug)]
pub(super) enum ConnectionSymmetry {
    /// We have only seen an incoming connection.
    IncomingOnly {
        /// Time this connection remained incoming only.
        since: Instant,
        /// The outgoing address of the peer that is connected to us.
        peer_addrs: BTreeSet<SocketAddr>,
    },
    /// We have only seen an outgoing connection.
    OutgoingOnly {
        /// Time this connection remained outgoing only.
        since: Instant,
    },
    /// The connection is fully symmetric.
    Symmetric {
        /// The outgoing address on the peer that is connected to us.
        peer_addrs: BTreeSet<SocketAddr>,
    },
    /// The connection is invalid/missing and should be removed.
    Gone,
}

impl Default for ConnectionSymmetry {
    fn default() -> Self {
        ConnectionSymmetry::Gone
    }
}

impl ConnectionSymmetry {
    /// A new incoming connection has been registered.
    ///
    /// Returns true, if the connection achieved symmetry with this change.
    pub(super) fn add_incoming(&mut self, peer_addr: SocketAddr, since: Instant) -> bool {
        match self {
            ConnectionSymmetry::IncomingOnly {
                ref mut peer_addrs, ..
            } => {
                // Already incoming connection, just add it to the pile.
                peer_addrs.insert(peer_addr);
                debug!(
                    total_incoming_count = peer_addrs.len(),
                    "added additional incoming connection on non-symmetric"
                );
                false
            }
            ConnectionSymmetry::OutgoingOnly { .. } => {
                // Outgoing graduates to Symmetric when we receive an incoming connection.
                let mut peer_addrs = BTreeSet::new();
                peer_addrs.insert(peer_addr);
                *self = ConnectionSymmetry::Symmetric { peer_addrs };
                debug!("added incoming connection, now symmetric");
                true
            }
            ConnectionSymmetry::Symmetric { peer_addrs } => {
                // Just record an additional incoming connection.
                peer_addrs.insert(peer_addr);
                debug!(
                    total_incoming_count = peer_addrs.len(),
                    "added additional incoming connection on symmetric"
                );
                false
            }
            ConnectionSymmetry::Gone => {
                let mut peer_addrs = BTreeSet::new();
                peer_addrs.insert(peer_addr);
                *self = ConnectionSymmetry::IncomingOnly { peer_addrs, since };
                debug!("added incoming connection, now incoming only");
                false
            }
        }
    }

    /// An incoming address has been removed.
    ///
    /// Returns `false` if the `ConnectionSymmetry` should be removed after this.
    pub(super) fn remove_incoming(&mut self, peer_addr: SocketAddr, now: Instant) -> bool {
        match self {
            ConnectionSymmetry::IncomingOnly { peer_addrs, .. } => {
                // Remove the incoming connection, warn if it didn't exist.
                if !peer_addrs.remove(&peer_addr) {
                    warn!("tried to remove non-existent incoming connection from symmetry");
                }

                // Indicate removal if this was the last incoming connection.
                if peer_addrs.is_empty() {
                    *self = ConnectionSymmetry::Gone;
                    debug!("removed incoming connection, now gone");

                    false
                } else {
                    debug!(
                        total_incoming_count = peer_addrs.len(),
                        "removed incoming connection, still has remaining incoming"
                    );

                    true
                }
            }
            ConnectionSymmetry::OutgoingOnly { .. } => {
                warn!("cannot remove incoming connection from outgoing-only");
                true
            }
            ConnectionSymmetry::Symmetric { peer_addrs } => {
                if !peer_addrs.remove(&peer_addr) {
                    warn!("tried to remove non-existent symmetric connection from symmetry");
                }
                if peer_addrs.is_empty() {
                    *self = ConnectionSymmetry::OutgoingOnly { since: now };
                    debug!("removed incoming connection, now incoming-only");
                }
                true
            }
            ConnectionSymmetry::Gone => {
                // This is just an error.
                warn!("removing incoming connection from already gone symmetry");
                false
            }
        }
    }

    /// Marks a connection as having an outgoing connection.
    ///
    /// Returns true, if the connection achieved symmetry with this change.
    pub(super) fn mark_outgoing(&mut self, now: Instant) -> bool {
        match self {
            ConnectionSymmetry::IncomingOnly { peer_addrs, .. } => {
                // Connection is now complete.
                debug!("incoming connection marked outgoing, now complete");
                *self = ConnectionSymmetry::Symmetric {
                    peer_addrs: mem::take(peer_addrs),
                };
                true
            }
            ConnectionSymmetry::OutgoingOnly { .. } => {
                warn!("outgoing connection marked outgoing");
                false
            }
            ConnectionSymmetry::Symmetric { .. } => {
                warn!("symmetric connection marked outgoing");
                false
            }
            ConnectionSymmetry::Gone => {
                *self = ConnectionSymmetry::OutgoingOnly { since: now };
                debug!("absent connection marked outgoing");
                false
            }
        }
    }

    /// Unmarks a connection as having an outgoing connection.
    ///
    /// Returns `false` if the `ConnectionSymmetry` should be removed after this.
    pub(super) fn unmark_outgoing(&mut self, now: Instant) -> bool {
        match self {
            ConnectionSymmetry::IncomingOnly { .. } => {
                warn!("incoming-only unmarked outgoing");
                true
            }
            ConnectionSymmetry::OutgoingOnly { .. } => {
                // With neither incoming, nor outgoing connections, the symmetry is finally gone.
                *self = ConnectionSymmetry::Gone;
                debug!("outgoing connection unmarked, now gone");

                false
            }
            ConnectionSymmetry::Symmetric { peer_addrs } => {
                *self = ConnectionSymmetry::IncomingOnly {
                    peer_addrs: mem::take(peer_addrs),
                    since: now,
                };
                debug!("symmetric connection unmarked, now outgoing only");

                true
            }
            ConnectionSymmetry::Gone => {
                warn!("gone marked outgoing");
                false
            }
        }
    }

    /// Indicates whether or not a connection should be cleaned up.
    pub(super) fn should_be_reaped(&self, now: Instant, max_time_asymmetric: Duration) -> bool {
        match self {
            ConnectionSymmetry::IncomingOnly { since, .. } => now >= *since + max_time_asymmetric,
            ConnectionSymmetry::OutgoingOnly { since } => now >= *since + max_time_asymmetric,
            ConnectionSymmetry::Symmetric { .. } => false,
            ConnectionSymmetry::Gone => true,
        }
    }

    /// Returns the set of incoming addresses, if any.
    pub(super) fn incoming_addrs(&self) -> Option<&BTreeSet<SocketAddr>> {
        match self {
            ConnectionSymmetry::IncomingOnly { peer_addrs, .. }
            | ConnectionSymmetry::Symmetric { peer_addrs, .. } => Some(peer_addrs),
            ConnectionSymmetry::OutgoingOnly { .. } | ConnectionSymmetry::Gone => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, net::SocketAddr, time::Duration};

    use crate::testing::test_clock::TestClock;

    use super::ConnectionSymmetry;

    #[test]
    fn symmetry_successful_lifecycles() {
        let mut clock = TestClock::new();

        let max_time_asymmetric = Duration::from_secs(240);
        let peer_addr: SocketAddr = "1.2.3.4:1234".parse().unwrap();

        let mut sym = ConnectionSymmetry::default();

        // Symmetries that have just been initialized are always reaped instantly.
        assert!(sym.should_be_reaped(clock.now(), max_time_asymmetric));

        // Adding an incoming address.
        sym.add_incoming(peer_addr, clock.now());
        assert!(!sym.should_be_reaped(clock.now(), max_time_asymmetric));

        // Add an outgoing address.
        clock.advance(Duration::from_secs(20));
        sym.mark_outgoing(clock.now());

        // The connection will now never be reaped, as it is symmetrical.
        clock.advance(Duration::from_secs(1_000_000));
        assert!(!sym.should_be_reaped(clock.now(), max_time_asymmetric));
    }

    #[test]
    fn symmetry_lifecycle_reaps_incoming_only() {
        let mut clock = TestClock::new();

        let max_time_asymmetric = Duration::from_secs(240);
        let peer_addr: SocketAddr = "1.2.3.4:1234".parse().unwrap();
        let peer_addr2: SocketAddr = "1.2.3.4:1234".parse().unwrap();

        let mut sym = ConnectionSymmetry::default();

        // Adding an incoming address prevents it from being reaped.
        sym.add_incoming(peer_addr, clock.now());
        assert!(!sym.should_be_reaped(clock.now(), max_time_asymmetric));

        // Adding another incoming address does not change the timeout.
        clock.advance(Duration::from_secs(120));
        sym.add_incoming(peer_addr2, clock.now());
        assert!(!sym.should_be_reaped(clock.now(), max_time_asymmetric));

        // We also expected `peer_addr` and `peer_addr2` to be the incoming addresses now.
        let mut expected = BTreeSet::new();
        expected.insert(peer_addr);
        expected.insert(peer_addr2);
        assert_eq!(sym.incoming_addrs(), Some(&expected));

        // After 240 seconds since the first incoming connection, we finally are due reaping.
        clock.advance(Duration::from_secs(120));
        assert!(sym.should_be_reaped(clock.now(), max_time_asymmetric));
    }

    #[test]
    fn symmetry_lifecycle_reaps_outgoing_only() {
        let mut clock = TestClock::new();

        let max_time_asymmetric = Duration::from_secs(240);

        let mut sym = ConnectionSymmetry::default();

        // Mark as outgoing, to prevent reaping.
        sym.mark_outgoing(clock.now());
        assert!(!sym.should_be_reaped(clock.now(), max_time_asymmetric));

        // Marking as outgoing again is usually an error, but should not affect the timeout.
        clock.advance(Duration::from_secs(120));
        assert!(!sym.should_be_reaped(clock.now(), max_time_asymmetric));

        // After 240 seconds we finally are reaping.
        clock.advance(Duration::from_secs(120));
        assert!(sym.should_be_reaped(clock.now(), max_time_asymmetric));
    }
}
