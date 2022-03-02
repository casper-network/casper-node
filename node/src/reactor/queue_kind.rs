//! Queue kinds.
//!
//! The reactor's event queue uses different queues to group events by priority and polls them in a
//! round-robin manner. This way, events are only competing for time within one queue, non-congested
//! queues can always assume to be speedily processed.

use std::{fmt::Display, num::NonZeroUsize};

use enum_iterator::IntoEnumIterator;
use serde::Serialize;

/// Scheduling priority.
///
/// Priorities are ordered from lowest to highest.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, IntoEnumIterator, PartialOrd, Ord, Serialize)]
pub enum QueueKind {
    /// Control messages for the runtime itself.
    Control,
    /// Network events that were initiated outside of this node.
    ///
    /// Their load may vary and grouping them together in one queue aides DoS protection.
    NetworkIncoming,
    /// Network events that are low priority.
    NetworkLowPriority,
    /// Network events that were initiated by the local node, such as outgoing messages.
    Network,
    /// Events of unspecified priority.
    ///
    /// This is the default queue.
    Regular,
    /// Reporting events on the local node.
    ///
    /// Metric events take precedence over most other events since missing a request for metrics
    /// might cause the requester to assume that the node is down and forcefully restart it.
    Api,
}

impl Display for QueueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str_value = match self {
            QueueKind::Control => "Control",
            QueueKind::NetworkIncoming => "NetworkIncoming",
            QueueKind::NetworkLowPriority => "NetworkLowPriority",
            QueueKind::Network => "Network",
            QueueKind::Regular => "Regular",
            QueueKind::Api => "Api",
        };
        write!(f, "{}", str_value)
    }
}

impl Default for QueueKind {
    fn default() -> Self {
        QueueKind::Regular
    }
}

impl QueueKind {
    /// Returns the weight of a specific queue.
    ///
    /// The weight determines how many events are at most processed from a specific queue during
    /// each event processing round.
    fn weight(self) -> NonZeroUsize {
        NonZeroUsize::new(match self {
            // Note: Control events should be very rare, but we do want to process them right away.
            QueueKind::Control => 32,
            QueueKind::NetworkIncoming => 4,
            QueueKind::NetworkLowPriority => 1,
            QueueKind::Network => 4,
            QueueKind::Regular => 8,
            QueueKind::Api => 16,
        })
        .expect("weight must be positive")
    }

    /// Return weights of all possible `Queue`s.
    pub(crate) fn weights() -> Vec<(Self, NonZeroUsize)> {
        QueueKind::into_enum_iter()
            .map(|q| (q, q.weight()))
            .collect()
    }

    pub(crate) fn metrics_name(&self) -> &str {
        match self {
            QueueKind::Control => "control",
            QueueKind::NetworkIncoming => "network_incoming",
            QueueKind::NetworkLowPriority => "network_low_priority",
            QueueKind::Network => "network",
            QueueKind::Regular => "regular",
            QueueKind::Api => "api",
        }
    }
}
