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
    /// Network events demand a resource directly.
    NetworkDemand,
    /// Network events that were initiated by the local node, such as outgoing messages.
    Network,
    /// NetworkInfo events.
    NetworkInfo,
    /// Fetch events.
    Fetch,
    /// SyncGlobalState events.
    SyncGlobalState,
    /// FinalitySignature events.
    FinalitySignature,
    /// Events of unspecified priority.
    ///
    /// This is the default queue.
    Regular,
    /// Gossiper events.
    Gossip,
    /// Get from storage events.
    FromStorage,
    /// Put to storage events.
    ToStorage,
    /// Contract runtime events.
    ContractRuntime,
    /// Consensus events.
    Consensus,
    /// Validation events.
    Validation,
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
            QueueKind::NetworkDemand => "NetworkDemand",
            QueueKind::Network => "Network",
            QueueKind::NetworkInfo => "NetworkInfo",
            QueueKind::Fetch => "Fetch",
            QueueKind::Regular => "Regular",
            QueueKind::Gossip => "Gossip",
            QueueKind::FromStorage => "FromStorage",
            QueueKind::ToStorage => "ToStorage",
            QueueKind::ContractRuntime => "ContractRuntime",
            QueueKind::SyncGlobalState => "SyncGlobalState",
            QueueKind::FinalitySignature => "FinalitySignature",
            QueueKind::Consensus => "Consensus",
            QueueKind::Validation => "Validation",
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
            QueueKind::NetworkLowPriority => 1,
            QueueKind::NetworkInfo => 2,
            QueueKind::NetworkDemand => 2,
            QueueKind::NetworkIncoming => 4,
            QueueKind::Network => 4,
            QueueKind::Regular => 4,
            QueueKind::Fetch => 4,
            QueueKind::Gossip => 4,
            QueueKind::FromStorage => 4,
            QueueKind::ToStorage => 4,
            QueueKind::ContractRuntime => 4,
            QueueKind::SyncGlobalState => 4,
            QueueKind::Consensus => 4,
            QueueKind::FinalitySignature => 4,
            QueueKind::Validation => 8,
            QueueKind::Api => 8,
            // Note: Control events should be very rare, but we do want to process them right away.
            QueueKind::Control => 16,
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
            QueueKind::NetworkDemand => "network_demands",
            QueueKind::NetworkLowPriority => "network_low_priority",
            QueueKind::Network => "network",
            QueueKind::NetworkInfo => "network_info",
            QueueKind::SyncGlobalState => "sync_global_state",
            QueueKind::Fetch => "fetch",
            QueueKind::Gossip => "gossip",
            QueueKind::FromStorage => "from_storage",
            QueueKind::ToStorage => "to_storage",
            QueueKind::ContractRuntime => "contract_runtime",
            QueueKind::Consensus => "consensus",
            QueueKind::Validation => "validation",
            QueueKind::FinalitySignature => "finality_signature",
            QueueKind::Api => "api",
            QueueKind::Regular => "regular",
        }
    }
}
