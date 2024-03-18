//! Queue kinds.
//!
//! The reactor's event queue uses different queues to group events by priority and polls them in a
//! round-robin manner. This way, events are only competing for time within one queue, non-congested
//! queues can always assume to be speedily processed.

use std::num::NonZeroUsize;

use enum_iterator::IntoEnumIterator;
use serde::Serialize;

/// Scheduling priority.
///
/// Priorities are ordered from lowest to highest.
#[derive(
    Copy,
    Clone,
    Debug,
    strum::Display,
    Eq,
    PartialEq,
    Hash,
    IntoEnumIterator,
    PartialOrd,
    Ord,
    Serialize,
    Default,
)]
pub enum QueueKind {
    /// Control messages for the runtime itself.
    Control,
    /// Incoming message events that were initiated outside of this node.
    ///
    /// Their load may vary and grouping them together in one queue aids DoS protection.
    MessageIncoming,
    /// Incoming messages that are low priority.
    MessageLowPriority,
    /// Incoming messages from validators.
    MessageValidator,
    /// Outgoing messages.
    NetworkOutgoing,
    /// NetworkInfo events.
    NetworkInfo,
    /// Internal network events.
    NetworkInternal,
    /// Fetch events.
    Fetch,
    /// SyncGlobalState events.
    SyncGlobalState,
    /// FinalitySignature events.
    FinalitySignature,
    /// Events of unspecified priority.
    ///
    /// This is the default queue.
    #[default]
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

impl QueueKind {
    /// Returns the weight of a specific queue.
    ///
    /// The weight determines how many events are at most processed from a specific queue during
    /// each event processing round.
    fn weight(self) -> NonZeroUsize {
        NonZeroUsize::new(match self {
            QueueKind::MessageLowPriority => 1,
            QueueKind::NetworkInfo => 2,
            QueueKind::MessageIncoming => 4,
            QueueKind::MessageValidator => 8,
            QueueKind::NetworkOutgoing => 4,
            QueueKind::NetworkInternal => 4,
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
            QueueKind::MessageIncoming => "message_incoming",
            QueueKind::MessageLowPriority => "message_low_priority",
            QueueKind::MessageValidator => "message_validator",
            QueueKind::NetworkOutgoing => "network_outgoing",
            QueueKind::NetworkInfo => "network_info",
            QueueKind::NetworkInternal => "network_internal",
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
