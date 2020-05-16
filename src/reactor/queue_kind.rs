//! Queue kinds
//!
//! The reactor's event queue uses different queues to group events by priority and polls them in a
//! round-robin manner. This way, events are only competing for time within one queue, non-congested
//! queues can always assume to be speedily processed.

use enum_iterator::IntoEnumIterator;
use std::num;

/// Scheduling priority.
///
/// Priorities are ordered from lowest to highest.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, IntoEnumIterator)]
pub enum Queue {
    /// Network events that were initiated outside of this node.
    ///
    /// Their load may vary and grouping them together in one queue aides DoS protection.
    NetworkIncoming,
    /// Network events that were initiated by the local node, such as outgoing messages.
    Network,
    /// Events of unspecified priority.
    ///
    /// This is the default queue.
    Regular,
    /// Reporting events on the local node.
    ///
    /// Metric events take precendence over most other events since missing a request for metrics
    /// might cause the requester to assume that the node is down and forcefully restart it.
    Metrics,
}

impl Default for Queue {
    fn default() -> Self {
        Queue::Regular
    }
}

impl Queue {
    /// Return the weight of a specific queue.
    ///
    /// The weight determines how many events are at most processed from a specific queue during
    /// each event processing round.
    fn weight(&self) -> num::NonZeroUsize {
        num::NonZeroUsize::new(match self {
            Queue::NetworkIncoming => 4,
            Queue::Network => 4,
            Queue::Regular => 8,
            Queue::Metrics => 16,
        })
        .expect("weight must be positive")
    }

    /// Return weights of all possible `Queue`s.
    pub(super) fn weights() -> Vec<(Self, num::NonZeroUsize)> {
        Queue::into_enum_iter().map(|q| (q, q.weight())).collect()
    }
}
