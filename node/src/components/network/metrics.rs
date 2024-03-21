use prometheus::{IntCounter, IntGauge, Registry};

use crate::utils::registered_metric::{DeprecatedMetric, RegisteredMetric, RegistryExt};

/// Network-type agnostic networking metrics.
#[derive(Debug)]
#[allow(dead_code)] // TODO: Remove this once deprecated metrics are removed.
pub(super) struct Metrics {
    /// Number of broadcasts attempted.
    pub(super) broadcast_requests: RegisteredMetric<IntCounter>,
    /// Number of gossips sent.
    pub(super) gossip_requests: RegisteredMetric<IntCounter>,
    /// Number of directly sent messages.
    pub(super) direct_message_requests: RegisteredMetric<IntCounter>,
    /// Number of connected peers.
    pub(super) peers: RegisteredMetric<IntGauge>,
    /// How many additional messages have been buffered outside of the juliet stack.
    pub(super) overflow_buffer_count: RegisteredMetric<IntGauge>,
    /// How many additional payload bytes have been buffered outside of the juliet stack.
    pub(super) overflow_buffer_bytes: RegisteredMetric<IntGauge>,

    // *** Deprecated metrics below ***
    /// Number of messages still waiting to be sent out (broadcast and direct).
    pub(super) queued_messages: DeprecatedMetric,
    /// Count of outgoing messages that are protocol overhead.
    pub(super) out_count_protocol: DeprecatedMetric,
    /// Count of outgoing messages with consensus payload.
    pub(super) out_count_consensus: DeprecatedMetric,
    /// Count of outgoing messages with deploy gossiper payload.
    pub(super) out_count_deploy_gossip: DeprecatedMetric,
    pub(super) out_count_block_gossip: DeprecatedMetric,
    pub(super) out_count_finality_signature_gossip: DeprecatedMetric,
    /// Count of outgoing messages with address gossiper payload.
    pub(super) out_count_address_gossip: DeprecatedMetric,
    /// Count of outgoing messages with deploy request/response payload.
    pub(super) out_count_deploy_transfer: DeprecatedMetric,
    /// Count of outgoing messages with block request/response payload.
    pub(super) out_count_block_transfer: DeprecatedMetric,
    /// Count of outgoing messages with trie request/response payload.
    pub(super) out_count_trie_transfer: DeprecatedMetric,
    /// Count of outgoing messages with other payload.
    pub(super) out_count_other: DeprecatedMetric,
    /// Volume in bytes of outgoing messages that are protocol overhead.
    pub(super) out_bytes_protocol: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with consensus payload.
    pub(super) out_bytes_consensus: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with deploy gossiper payload.
    pub(super) out_bytes_deploy_gossip: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with block gossiper payload.
    pub(super) out_bytes_block_gossip: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with finality signature payload.
    pub(super) out_bytes_finality_signature_gossip: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with address gossiper payload.
    pub(super) out_bytes_address_gossip: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with deploy request/response payload.
    pub(super) out_bytes_deploy_transfer: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with block request/response payload.
    pub(super) out_bytes_block_transfer: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with block request/response payload.
    pub(super) out_bytes_trie_transfer: DeprecatedMetric,
    /// Volume in bytes of outgoing messages with other payload.
    pub(super) out_bytes_other: DeprecatedMetric,
    /// Number of outgoing connections in connecting state.
    pub(super) out_state_connecting: DeprecatedMetric,
    /// Number of outgoing connections in waiting state.
    pub(super) out_state_waiting: DeprecatedMetric,
    /// Number of outgoing connections in connected state.
    pub(super) out_state_connected: DeprecatedMetric,
    /// Number of outgoing connections in blocked state.
    pub(super) out_state_blocked: DeprecatedMetric,
    /// Number of outgoing connections in loopback state.
    pub(super) out_state_loopback: DeprecatedMetric,
    /// Volume in bytes of incoming messages that are protocol overhead.
    pub(super) in_bytes_protocol: DeprecatedMetric,
    /// Volume in bytes of incoming messages with consensus payload.
    pub(super) in_bytes_consensus: DeprecatedMetric,
    /// Volume in bytes of incoming messages with deploy gossiper payload.
    pub(super) in_bytes_deploy_gossip: DeprecatedMetric,
    /// Volume in bytes of incoming messages with block gossiper payload.
    pub(super) in_bytes_block_gossip: DeprecatedMetric,
    /// Volume in bytes of incoming messages with finality signature gossiper payload.
    pub(super) in_bytes_finality_signature_gossip: DeprecatedMetric,
    /// Volume in bytes of incoming messages with address gossiper payload.
    pub(super) in_bytes_address_gossip: DeprecatedMetric,
    /// Volume in bytes of incoming messages with deploy request/response payload.
    pub(super) in_bytes_deploy_transfer: DeprecatedMetric,
    /// Volume in bytes of incoming messages with block request/response payload.
    pub(super) in_bytes_block_transfer: DeprecatedMetric,
    /// Volume in bytes of incoming messages with block request/response payload.
    pub(super) in_bytes_trie_transfer: DeprecatedMetric,
    /// Volume in bytes of incoming messages with other payload.
    pub(super) in_bytes_other: DeprecatedMetric,
    /// Count of incoming messages that are protocol overhead.
    pub(super) in_count_protocol: DeprecatedMetric,
    /// Count of incoming messages with consensus payload.
    pub(super) in_count_consensus: DeprecatedMetric,
    /// Count of incoming messages with deploy gossiper payload.
    pub(super) in_count_deploy_gossip: DeprecatedMetric,
    /// Count of incoming messages with block gossiper payload.
    pub(super) in_count_block_gossip: DeprecatedMetric,
    /// Count of incoming messages with finality signature gossiper payload.
    pub(super) in_count_finality_signature_gossip: DeprecatedMetric,
    /// Count of incoming messages with address gossiper payload.
    pub(super) in_count_address_gossip: DeprecatedMetric,
    /// Count of incoming messages with deploy request/response payload.
    pub(super) in_count_deploy_transfer: DeprecatedMetric,
    /// Count of incoming messages with block request/response payload.
    pub(super) in_count_block_transfer: DeprecatedMetric,
    /// Count of incoming messages with trie request/response payload.
    pub(super) in_count_trie_transfer: DeprecatedMetric,
    /// Count of incoming messages with other payload.
    pub(super) in_count_other: DeprecatedMetric,
    /// Number of trie requests accepted for processing.
    pub(super) requests_for_trie_accepted: DeprecatedMetric,
    /// Number of trie requests finished (successful or unsuccessful).
    pub(super) requests_for_trie_finished: DeprecatedMetric,
    /// Total time spent delaying outgoing traffic to non-validators due to limiter, in seconds.
    pub(super) accumulated_outgoing_limiter_delay: DeprecatedMetric,
}

impl Metrics {
    /// Creates a new instance of networking metrics.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let broadcast_requests =
            registry.new_int_counter("net_broadcast_requests", "number of broadcasts attempted")?;
        let gossip_requests =
            registry.new_int_counter("net_gossip_requests", "number of gossips sent")?;
        let direct_message_requests = registry.new_int_counter(
            "net_direct_message_requests",
            "number of requests to send a message directly to a peer",
        )?;

        let peers = registry.new_int_gauge("peers", "number of connected peers")?;

        let overflow_buffer_count = registry.new_int_gauge(
            "net_overflow_buffer_count",
            "count of outgoing messages buffered outside network stack",
        )?;
        let overflow_buffer_bytes = registry.new_int_gauge(
            "net_overflow_buffer_bytes",
            "payload byte sum of outgoing messages buffered outside network stack",
        )?;

        // *** Deprecated metrics below ***
        let queued_messages = registry.new_deprecated(
            "net_queued_direct_messages",
            "number of messages waiting to be sent out",
        )?;
        let out_count_protocol = registry.new_deprecated(
            "net_out_count_protocol",
            "count of outgoing messages that are protocol overhead",
        )?;
        let out_count_consensus = registry.new_deprecated(
            "net_out_count_consensus",
            "count of outgoing messages with consensus payload",
        )?;
        let out_count_deploy_gossip = registry.new_deprecated(
            "net_out_count_deploy_gossip",
            "count of outgoing messages with deploy gossiper payload",
        )?;
        let out_count_block_gossip = registry.new_deprecated(
            "net_out_count_block_gossip",
            "count of outgoing messages with block gossiper payload",
        )?;
        let out_count_finality_signature_gossip = registry.new_deprecated(
            "net_out_count_finality_signature_gossip",
            "count of outgoing messages with finality signature gossiper payload",
        )?;
        let out_count_address_gossip = registry.new_deprecated(
            "net_out_count_address_gossip",
            "count of outgoing messages with address gossiper payload",
        )?;
        let out_count_deploy_transfer = registry.new_deprecated(
            "net_out_count_deploy_transfer",
            "count of outgoing messages with deploy request/response payload",
        )?;
        let out_count_block_transfer = registry.new_deprecated(
            "net_out_count_block_transfer",
            "count of outgoing messages with block request/response payload",
        )?;
        let out_count_trie_transfer = registry.new_deprecated(
            "net_out_count_trie_transfer",
            "count of outgoing messages with trie payloads",
        )?;
        let out_count_other = registry.new_deprecated(
            "net_out_count_other",
            "count of outgoing messages with other payload",
        )?;

        let out_bytes_protocol = registry.new_deprecated(
            "net_out_bytes_protocol",
            "volume in bytes of outgoing messages that are protocol overhead",
        )?;
        let out_bytes_consensus = registry.new_deprecated(
            "net_out_bytes_consensus",
            "volume in bytes of outgoing messages with consensus payload",
        )?;
        let out_bytes_deploy_gossip = registry.new_deprecated(
            "net_out_bytes_deploy_gossip",
            "volume in bytes of outgoing messages with deploy gossiper payload",
        )?;
        let out_bytes_block_gossip = registry.new_deprecated(
            "net_out_bytes_block_gossip",
            "volume in bytes of outgoing messages with block gossiper payload",
        )?;
        let out_bytes_finality_signature_gossip = registry.new_deprecated(
            "net_out_bytes_finality_signature_gossip",
            "volume in bytes of outgoing messages with finality signature gossiper payload",
        )?;
        let out_bytes_address_gossip = registry.new_deprecated(
            "net_out_bytes_address_gossip",
            "volume in bytes of outgoing messages with address gossiper payload",
        )?;
        let out_bytes_deploy_transfer = registry.new_deprecated(
            "net_out_bytes_deploy_transfer",
            "volume in bytes of outgoing messages with deploy request/response payload",
        )?;
        let out_bytes_block_transfer = registry.new_deprecated(
            "net_out_bytes_block_transfer",
            "volume in bytes of outgoing messages with block request/response payload",
        )?;
        let out_bytes_trie_transfer = registry.new_deprecated(
            "net_out_bytes_trie_transfer",
            "volume in bytes of outgoing messages with trie payloads",
        )?;
        let out_bytes_other = registry.new_deprecated(
            "net_out_bytes_other",
            "volume in bytes of outgoing messages with other payload",
        )?;

        let out_state_connecting = registry.new_deprecated(
            "out_state_connecting",
            "number of connections in the connecting state",
        )?;
        let out_state_waiting = registry.new_deprecated(
            "out_state_waiting",
            "number of connections in the waiting state",
        )?;
        let out_state_connected = registry.new_deprecated(
            "out_state_connected",
            "number of connections in the connected state",
        )?;
        let out_state_blocked = registry.new_deprecated(
            "out_state_blocked",
            "number of connections in the blocked state",
        )?;
        let out_state_loopback = registry.new_deprecated(
            "out_state_loopback",
            "number of connections in the loopback state",
        )?;

        let in_count_protocol = registry.new_deprecated(
            "net_in_count_protocol",
            "count of incoming messages that are protocol overhead",
        )?;
        let in_count_consensus = registry.new_deprecated(
            "net_in_count_consensus",
            "count of incoming messages with consensus payload",
        )?;
        let in_count_deploy_gossip = registry.new_deprecated(
            "net_in_count_deploy_gossip",
            "count of incoming messages with deploy gossiper payload",
        )?;
        let in_count_block_gossip = registry.new_deprecated(
            "net_in_count_block_gossip",
            "count of incoming messages with block gossiper payload",
        )?;
        let in_count_finality_signature_gossip = registry.new_deprecated(
            "net_in_count_finality_signature_gossip",
            "count of incoming messages with finality signature gossiper payload",
        )?;
        let in_count_address_gossip = registry.new_deprecated(
            "net_in_count_address_gossip",
            "count of incoming messages with address gossiper payload",
        )?;
        let in_count_deploy_transfer = registry.new_deprecated(
            "net_in_count_deploy_transfer",
            "count of incoming messages with deploy request/response payload",
        )?;
        let in_count_block_transfer = registry.new_deprecated(
            "net_in_count_block_transfer",
            "count of incoming messages with block request/response payload",
        )?;
        let in_count_trie_transfer = registry.new_deprecated(
            "net_in_count_trie_transfer",
            "count of incoming messages with trie payloads",
        )?;
        let in_count_other = registry.new_deprecated(
            "net_in_count_other",
            "count of incoming messages with other payload",
        )?;

        let in_bytes_protocol = registry.new_deprecated(
            "net_in_bytes_protocol",
            "volume in bytes of incoming messages that are protocol overhead",
        )?;
        let in_bytes_consensus = registry.new_deprecated(
            "net_in_bytes_consensus",
            "volume in bytes of incoming messages with consensus payload",
        )?;
        let in_bytes_deploy_gossip = registry.new_deprecated(
            "net_in_bytes_deploy_gossip",
            "volume in bytes of incoming messages with deploy gossiper payload",
        )?;
        let in_bytes_block_gossip = registry.new_deprecated(
            "net_in_bytes_block_gossip",
            "volume in bytes of incoming messages with block gossiper payload",
        )?;
        let in_bytes_finality_signature_gossip = registry.new_deprecated(
            "net_in_bytes_finality_signature_gossip",
            "volume in bytes of incoming messages with finality signature gossiper payload",
        )?;
        let in_bytes_address_gossip = registry.new_deprecated(
            "net_in_bytes_address_gossip",
            "volume in bytes of incoming messages with address gossiper payload",
        )?;
        let in_bytes_deploy_transfer = registry.new_deprecated(
            "net_in_bytes_deploy_transfer",
            "volume in bytes of incoming messages with deploy request/response payload",
        )?;
        let in_bytes_block_transfer = registry.new_deprecated(
            "net_in_bytes_block_transfer",
            "volume in bytes of incoming messages with block request/response payload",
        )?;
        let in_bytes_trie_transfer = registry.new_deprecated(
            "net_in_bytes_trie_transfer",
            "volume in bytes of incoming messages with trie payloads",
        )?;
        let in_bytes_other = registry.new_deprecated(
            "net_in_bytes_other",
            "volume in bytes of incoming messages with other payload",
        )?;

        let requests_for_trie_accepted = registry.new_deprecated(
            "requests_for_trie_accepted",
            "number of trie requests accepted for processing",
        )?;
        let requests_for_trie_finished = registry.new_deprecated(
            "requests_for_trie_finished",
            "number of trie requests finished, successful or not",
        )?;

        let accumulated_outgoing_limiter_delay = registry.new_deprecated(
            "accumulated_outgoing_limiter_delay",
            "seconds spent delaying outgoing traffic to non-validators due to limiter, in seconds",
        )?;

        Ok(Metrics {
            broadcast_requests,
            gossip_requests,
            direct_message_requests,
            overflow_buffer_count,
            overflow_buffer_bytes,
            peers,
            queued_messages,
            out_count_protocol,
            out_count_consensus,
            out_count_deploy_gossip,
            out_count_block_gossip,
            out_count_finality_signature_gossip,
            out_count_address_gossip,
            out_count_deploy_transfer,
            out_count_block_transfer,
            out_count_trie_transfer,
            out_count_other,
            out_bytes_protocol,
            out_bytes_consensus,
            out_bytes_deploy_gossip,
            out_bytes_block_gossip,
            out_bytes_finality_signature_gossip,
            out_bytes_address_gossip,
            out_bytes_deploy_transfer,
            out_bytes_block_transfer,
            out_bytes_trie_transfer,
            out_bytes_other,
            out_state_connecting,
            out_state_waiting,
            out_state_connected,
            out_state_blocked,
            out_state_loopback,
            in_count_protocol,
            in_count_consensus,
            in_count_deploy_gossip,
            in_count_block_gossip,
            in_count_finality_signature_gossip,
            in_count_address_gossip,
            in_count_deploy_transfer,
            in_count_block_transfer,
            in_count_trie_transfer,
            in_count_other,
            in_bytes_protocol,
            in_bytes_consensus,
            in_bytes_deploy_gossip,
            in_bytes_block_gossip,
            in_bytes_finality_signature_gossip,
            in_bytes_address_gossip,
            in_bytes_deploy_transfer,
            in_bytes_block_transfer,
            in_bytes_trie_transfer,
            in_bytes_other,
            requests_for_trie_accepted,
            requests_for_trie_finished,
            accumulated_outgoing_limiter_delay,
        })
    }
}
