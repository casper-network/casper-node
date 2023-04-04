use std::sync::Weak;

use prometheus::{Counter, IntCounter, IntGauge, Registry};
use tracing::debug;

use super::{outgoing::OutgoingMetrics, MessageKind};
use crate::unregister_metric;

/// Network-type agnostic networking metrics.
#[derive(Debug)]
pub(super) struct Metrics {
    /// How often a request was made by a component to broadcast.
    pub(super) broadcast_requests: IntCounter,
    /// How often a request to send a message directly to a peer was made.
    pub(super) direct_message_requests: IntCounter,
    /// Number of messages still waiting to be sent out (broadcast and direct).
    pub(super) queued_messages: IntGauge,
    /// Number of connected peers.
    pub(super) peers: IntGauge,

    /// Count of outgoing messages that are protocol overhead.
    pub(super) out_count_protocol: IntCounter,
    /// Count of outgoing messages with consensus payload.
    pub(super) out_count_consensus: IntCounter,
    /// Count of outgoing messages with deploy gossiper payload.
    pub(super) out_count_deploy_gossip: IntCounter,
    pub(super) out_count_block_gossip: IntCounter,
    pub(super) out_count_finality_signature_gossip: IntCounter,
    /// Count of outgoing messages with address gossiper payload.
    pub(super) out_count_address_gossip: IntCounter,
    /// Count of outgoing messages with deploy request/response payload.
    pub(super) out_count_deploy_transfer: IntCounter,
    /// Count of outgoing messages with block request/response payload.
    pub(super) out_count_block_transfer: IntCounter,
    /// Count of outgoing messages with trie request/response payload.
    pub(super) out_count_trie_transfer: IntCounter,
    /// Count of outgoing messages with other payload.
    pub(super) out_count_other: IntCounter,

    /// Volume in bytes of outgoing messages that are protocol overhead.
    pub(super) out_bytes_protocol: IntCounter,
    /// Volume in bytes of outgoing messages with consensus payload.
    pub(super) out_bytes_consensus: IntCounter,
    /// Volume in bytes of outgoing messages with deploy gossiper payload.
    pub(super) out_bytes_deploy_gossip: IntCounter,
    pub(super) out_bytes_block_gossip: IntCounter,
    pub(super) out_bytes_finality_signature_gossip: IntCounter,
    /// Volume in bytes of outgoing messages with address gossiper payload.
    pub(super) out_bytes_address_gossip: IntCounter,
    /// Volume in bytes of outgoing messages with deploy request/response payload.
    pub(super) out_bytes_deploy_transfer: IntCounter,
    /// Volume in bytes of outgoing messages with block request/response payload.
    pub(super) out_bytes_block_transfer: IntCounter,
    /// Volume in bytes of outgoing messages with block request/response payload.
    pub(super) out_bytes_trie_transfer: IntCounter,
    /// Volume in bytes of outgoing messages with other payload.
    pub(super) out_bytes_other: IntCounter,

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

    /// Volume in bytes of incoming messages that are protocol overhead.
    pub(super) in_bytes_protocol: IntCounter,
    /// Volume in bytes of incoming messages with consensus payload.
    pub(super) in_bytes_consensus: IntCounter,
    /// Volume in bytes of incoming messages with deploy gossiper payload.
    pub(super) in_bytes_deploy_gossip: IntCounter,
    pub(super) in_bytes_block_gossip: IntCounter,
    pub(super) in_bytes_finality_signature_gossip: IntCounter,
    /// Volume in bytes of incoming messages with address gossiper payload.
    pub(super) in_bytes_address_gossip: IntCounter,
    /// Volume in bytes of incoming messages with deploy request/response payload.
    pub(super) in_bytes_deploy_transfer: IntCounter,
    /// Volume in bytes of incoming messages with block request/response payload.
    pub(super) in_bytes_block_transfer: IntCounter,
    /// Volume in bytes of incoming messages with block request/response payload.
    pub(super) in_bytes_trie_transfer: IntCounter,
    /// Volume in bytes of incoming messages with other payload.
    pub(super) in_bytes_other: IntCounter,

    /// Count of incoming messages that are protocol overhead.
    pub(super) in_count_protocol: IntCounter,
    /// Count of incoming messages with consensus payload.
    pub(super) in_count_consensus: IntCounter,
    /// Count of incoming messages with deploy gossiper payload.
    pub(super) in_count_deploy_gossip: IntCounter,
    pub(super) in_count_block_gossip: IntCounter,
    pub(super) in_count_finality_signature_gossip: IntCounter,
    /// Count of incoming messages with address gossiper payload.
    pub(super) in_count_address_gossip: IntCounter,
    /// Count of incoming messages with deploy request/response payload.
    pub(super) in_count_deploy_transfer: IntCounter,
    /// Count of incoming messages with block request/response payload.
    pub(super) in_count_block_transfer: IntCounter,
    /// Count of incoming messages with trie request/response payload.
    pub(super) in_count_trie_transfer: IntCounter,
    /// Count of incoming messages with other payload.
    pub(super) in_count_other: IntCounter,

    /// Number of trie requests accepted for processing.
    pub(super) requests_for_trie_accepted: IntCounter,
    /// Number of trie requests finished (successful or unsuccessful).
    pub(super) requests_for_trie_finished: IntCounter,

    /// Total time spent delaying outgoing traffic to non-validators due to limiter, in seconds.
    pub(super) accumulated_outgoing_limiter_delay: Counter,
    /// Total time spent delaying incoming traffic from non-validators due to limiter, in seconds.
    pub(super) accumulated_incoming_limiter_delay: Counter,

    /// Registry instance.
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of networking metrics.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let broadcast_requests =
            IntCounter::new("net_broadcast_requests", "number of broadcasting requests")?;
        let direct_message_requests = IntCounter::new(
            "net_direct_message_requests",
            "number of requests to send a message directly to a peer",
        )?;
        let queued_messages = IntGauge::new(
            "net_queued_direct_messages",
            "number of messages waiting to be sent out",
        )?;
        let peers = IntGauge::new("peers", "number of connected peers")?;

        let out_count_protocol = IntCounter::new(
            "net_out_count_protocol",
            "count of outgoing messages that are protocol overhead",
        )?;
        let out_count_consensus = IntCounter::new(
            "net_out_count_consensus",
            "count of outgoing messages with consensus payload",
        )?;
        let out_count_deploy_gossip = IntCounter::new(
            "net_out_count_deploy_gossip",
            "count of outgoing messages with deploy gossiper payload",
        )?;
        let out_count_block_gossip = IntCounter::new(
            "net_out_count_block_gossip",
            "count of outgoing messages with block gossiper payload",
        )?;
        let out_count_finality_signature_gossip = IntCounter::new(
            "net_out_count_finality_signature_gossip",
            "count of outgoing messages with finality signature gossiper payload",
        )?;
        let out_count_address_gossip = IntCounter::new(
            "net_out_count_address_gossip",
            "count of outgoing messages with address gossiper payload",
        )?;
        let out_count_deploy_transfer = IntCounter::new(
            "net_out_count_deploy_transfer",
            "count of outgoing messages with deploy request/response payload",
        )?;
        let out_count_block_transfer = IntCounter::new(
            "net_out_count_block_transfer",
            "count of outgoing messages with block request/response payload",
        )?;
        let out_count_trie_transfer = IntCounter::new(
            "net_out_count_trie_transfer",
            "count of outgoing messages with trie payloads",
        )?;
        let out_count_other = IntCounter::new(
            "net_out_count_other",
            "count of outgoing messages with other payload",
        )?;

        let out_bytes_protocol = IntCounter::new(
            "net_out_bytes_protocol",
            "volume in bytes of outgoing messages that are protocol overhead",
        )?;
        let out_bytes_consensus = IntCounter::new(
            "net_out_bytes_consensus",
            "volume in bytes of outgoing messages with consensus payload",
        )?;
        let out_bytes_deploy_gossip = IntCounter::new(
            "net_out_bytes_deploy_gossip",
            "volume in bytes of outgoing messages with deploy gossiper payload",
        )?;
        let out_bytes_block_gossip = IntCounter::new(
            "net_out_bytes_block_gossip",
            "volume in bytes of outgoing messages with block gossiper payload",
        )?;
        let out_bytes_finality_signature_gossip = IntCounter::new(
            "net_out_bytes_finality_signature_gossip",
            "volume in bytes of outgoing messages with finality signature gossiper payload",
        )?;
        let out_bytes_address_gossip = IntCounter::new(
            "net_out_bytes_address_gossip",
            "volume in bytes of outgoing messages with address gossiper payload",
        )?;
        let out_bytes_deploy_transfer = IntCounter::new(
            "net_out_bytes_deploy_transfer",
            "volume in bytes of outgoing messages with deploy request/response payload",
        )?;
        let out_bytes_block_transfer = IntCounter::new(
            "net_out_bytes_block_transfer",
            "volume in bytes of outgoing messages with block request/response payload",
        )?;
        let out_bytes_trie_transfer = IntCounter::new(
            "net_out_bytes_trie_transfer",
            "volume in bytes of outgoing messages with trie payloads",
        )?;
        let out_bytes_other = IntCounter::new(
            "net_out_bytes_other",
            "volume in bytes of outgoing messages with other payload",
        )?;

        let out_state_connecting = IntGauge::new(
            "out_state_connecting",
            "number of connections in the connecting state",
        )?;
        let out_state_waiting = IntGauge::new(
            "out_state_waiting",
            "number of connections in the waiting state",
        )?;
        let out_state_connected = IntGauge::new(
            "out_state_connected",
            "number of connections in the connected state",
        )?;
        let out_state_blocked = IntGauge::new(
            "out_state_blocked",
            "number of connections in the blocked state",
        )?;
        let out_state_loopback = IntGauge::new(
            "out_state_loopback",
            "number of connections in the loopback state",
        )?;

        let in_count_protocol = IntCounter::new(
            "net_in_count_protocol",
            "count of incoming messages that are protocol overhead",
        )?;
        let in_count_consensus = IntCounter::new(
            "net_in_count_consensus",
            "count of incoming messages with consensus payload",
        )?;
        let in_count_deploy_gossip = IntCounter::new(
            "net_in_count_deploy_gossip",
            "count of incoming messages with deploy gossiper payload",
        )?;
        let in_count_block_gossip = IntCounter::new(
            "net_in_count_block_gossip",
            "count of incoming messages with block gossiper payload",
        )?;
        let in_count_finality_signature_gossip = IntCounter::new(
            "net_in_count_finality_signature_gossip",
            "count of incoming messages with finality signature gossiper payload",
        )?;
        let in_count_address_gossip = IntCounter::new(
            "net_in_count_address_gossip",
            "count of incoming messages with address gossiper payload",
        )?;
        let in_count_deploy_transfer = IntCounter::new(
            "net_in_count_deploy_transfer",
            "count of incoming messages with deploy request/response payload",
        )?;
        let in_count_block_transfer = IntCounter::new(
            "net_in_count_block_transfer",
            "count of incoming messages with block request/response payload",
        )?;
        let in_count_trie_transfer = IntCounter::new(
            "net_in_count_trie_transfer",
            "count of incoming messages with trie payloads",
        )?;
        let in_count_other = IntCounter::new(
            "net_in_count_other",
            "count of incoming messages with other payload",
        )?;

        let in_bytes_protocol = IntCounter::new(
            "net_in_bytes_protocol",
            "volume in bytes of incoming messages that are protocol overhead",
        )?;
        let in_bytes_consensus = IntCounter::new(
            "net_in_bytes_consensus",
            "volume in bytes of incoming messages with consensus payload",
        )?;
        let in_bytes_deploy_gossip = IntCounter::new(
            "net_in_bytes_deploy_gossip",
            "volume in bytes of incoming messages with deploy gossiper payload",
        )?;
        let in_bytes_block_gossip = IntCounter::new(
            "net_in_bytes_block_gossip",
            "volume in bytes of incoming messages with block gossiper payload",
        )?;
        let in_bytes_finality_signature_gossip = IntCounter::new(
            "net_in_bytes_finality_signature_gossip",
            "volume in bytes of incoming messages with finality signature gossiper payload",
        )?;
        let in_bytes_address_gossip = IntCounter::new(
            "net_in_bytes_address_gossip",
            "volume in bytes of incoming messages with address gossiper payload",
        )?;
        let in_bytes_deploy_transfer = IntCounter::new(
            "net_in_bytes_deploy_transfer",
            "volume in bytes of incoming messages with deploy request/response payload",
        )?;
        let in_bytes_block_transfer = IntCounter::new(
            "net_in_bytes_block_transfer",
            "volume in bytes of incoming messages with block request/response payload",
        )?;
        let in_bytes_trie_transfer = IntCounter::new(
            "net_in_bytes_trie_transfer",
            "volume in bytes of incoming messages with trie payloads",
        )?;
        let in_bytes_other = IntCounter::new(
            "net_in_bytes_other",
            "volume in bytes of incoming messages with other payload",
        )?;

        let requests_for_trie_accepted = IntCounter::new(
            "requests_for_trie_accepted",
            "number of trie requests accepted for processing",
        )?;
        let requests_for_trie_finished = IntCounter::new(
            "requests_for_trie_finished",
            "number of trie requests finished, successful or not",
        )?;

        let accumulated_outgoing_limiter_delay = Counter::new(
            "accumulated_outgoing_limiter_delay",
            "seconds spent delaying outgoing traffic to non-validators due to limiter, in seconds",
        )?;
        let accumulated_incoming_limiter_delay = Counter::new(
            "accumulated_incoming_limiter_delay",
            "seconds spent delaying incoming traffic from non-validators due to limiter, in seconds."
        )?;

        registry.register(Box::new(broadcast_requests.clone()))?;
        registry.register(Box::new(direct_message_requests.clone()))?;
        registry.register(Box::new(queued_messages.clone()))?;
        registry.register(Box::new(peers.clone()))?;

        registry.register(Box::new(out_count_protocol.clone()))?;
        registry.register(Box::new(out_count_consensus.clone()))?;
        registry.register(Box::new(out_count_deploy_gossip.clone()))?;
        registry.register(Box::new(out_count_block_gossip.clone()))?;
        registry.register(Box::new(out_count_finality_signature_gossip.clone()))?;
        registry.register(Box::new(out_count_address_gossip.clone()))?;
        registry.register(Box::new(out_count_deploy_transfer.clone()))?;
        registry.register(Box::new(out_count_block_transfer.clone()))?;
        registry.register(Box::new(out_count_trie_transfer.clone()))?;
        registry.register(Box::new(out_count_other.clone()))?;

        registry.register(Box::new(out_bytes_protocol.clone()))?;
        registry.register(Box::new(out_bytes_consensus.clone()))?;
        registry.register(Box::new(out_bytes_deploy_gossip.clone()))?;
        registry.register(Box::new(out_bytes_block_gossip.clone()))?;
        registry.register(Box::new(out_bytes_finality_signature_gossip.clone()))?;
        registry.register(Box::new(out_bytes_address_gossip.clone()))?;
        registry.register(Box::new(out_bytes_deploy_transfer.clone()))?;
        registry.register(Box::new(out_bytes_block_transfer.clone()))?;
        registry.register(Box::new(out_bytes_trie_transfer.clone()))?;
        registry.register(Box::new(out_bytes_other.clone()))?;

        registry.register(Box::new(out_state_connecting.clone()))?;
        registry.register(Box::new(out_state_waiting.clone()))?;
        registry.register(Box::new(out_state_connected.clone()))?;
        registry.register(Box::new(out_state_blocked.clone()))?;
        registry.register(Box::new(out_state_loopback.clone()))?;

        registry.register(Box::new(in_count_protocol.clone()))?;
        registry.register(Box::new(in_count_consensus.clone()))?;
        registry.register(Box::new(in_count_deploy_gossip.clone()))?;
        registry.register(Box::new(in_count_block_gossip.clone()))?;
        registry.register(Box::new(in_count_finality_signature_gossip.clone()))?;
        registry.register(Box::new(in_count_address_gossip.clone()))?;
        registry.register(Box::new(in_count_deploy_transfer.clone()))?;
        registry.register(Box::new(in_count_block_transfer.clone()))?;
        registry.register(Box::new(in_count_trie_transfer.clone()))?;
        registry.register(Box::new(in_count_other.clone()))?;

        registry.register(Box::new(in_bytes_protocol.clone()))?;
        registry.register(Box::new(in_bytes_consensus.clone()))?;
        registry.register(Box::new(in_bytes_deploy_gossip.clone()))?;
        registry.register(Box::new(in_bytes_block_gossip.clone()))?;
        registry.register(Box::new(in_bytes_finality_signature_gossip.clone()))?;
        registry.register(Box::new(in_bytes_address_gossip.clone()))?;
        registry.register(Box::new(in_bytes_deploy_transfer.clone()))?;
        registry.register(Box::new(in_bytes_block_transfer.clone()))?;
        registry.register(Box::new(in_bytes_trie_transfer.clone()))?;
        registry.register(Box::new(in_bytes_other.clone()))?;

        registry.register(Box::new(requests_for_trie_accepted.clone()))?;
        registry.register(Box::new(requests_for_trie_finished.clone()))?;

        registry.register(Box::new(accumulated_outgoing_limiter_delay.clone()))?;
        registry.register(Box::new(accumulated_incoming_limiter_delay.clone()))?;

        Ok(Metrics {
            broadcast_requests,
            direct_message_requests,
            queued_messages,
            peers,
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
            accumulated_incoming_limiter_delay,
            registry: registry.clone(),
        })
    }

    /// Records an outgoing payload.
    pub(crate) fn record_payload_out(this: &Weak<Self>, kind: MessageKind, size: u64) {
        if let Some(metrics) = this.upgrade() {
            match kind {
                MessageKind::Protocol => {
                    metrics.out_bytes_protocol.inc_by(size);
                    metrics.out_count_protocol.inc();
                }
                MessageKind::Consensus => {
                    metrics.out_bytes_consensus.inc_by(size);
                    metrics.out_count_consensus.inc();
                }
                MessageKind::DeployGossip => {
                    metrics.out_bytes_deploy_gossip.inc_by(size);
                    metrics.out_count_deploy_gossip.inc();
                }
                MessageKind::BlockGossip => {
                    metrics.out_bytes_block_gossip.inc_by(size);
                    metrics.out_count_block_gossip.inc()
                }
                MessageKind::FinalitySignatureGossip => {
                    metrics.out_bytes_finality_signature_gossip.inc_by(size);
                    metrics.out_count_finality_signature_gossip.inc()
                }
                MessageKind::AddressGossip => {
                    metrics.out_bytes_address_gossip.inc_by(size);
                    metrics.out_count_address_gossip.inc();
                }
                MessageKind::DeployTransfer => {
                    metrics.out_bytes_deploy_transfer.inc_by(size);
                    metrics.out_count_deploy_transfer.inc();
                }
                MessageKind::BlockTransfer => {
                    metrics.out_bytes_block_transfer.inc_by(size);
                    metrics.out_count_block_transfer.inc();
                }
                MessageKind::TrieTransfer => {
                    metrics.out_bytes_trie_transfer.inc_by(size);
                    metrics.out_count_trie_transfer.inc();
                }
                MessageKind::Other => {
                    metrics.out_bytes_other.inc_by(size);
                    metrics.out_count_other.inc();
                }
            }
        } else {
            debug!("not recording metrics, component already shut down");
        }
    }

    /// Records an incoming payload.
    pub(crate) fn record_payload_in(this: &Weak<Self>, kind: MessageKind, size: u64) {
        if let Some(metrics) = this.upgrade() {
            match kind {
                MessageKind::Protocol => {
                    metrics.in_bytes_protocol.inc_by(size);
                    metrics.in_count_protocol.inc();
                }
                MessageKind::Consensus => {
                    metrics.in_bytes_consensus.inc_by(size);
                    metrics.in_count_consensus.inc();
                }
                MessageKind::DeployGossip => {
                    metrics.in_bytes_deploy_gossip.inc_by(size);
                    metrics.in_count_deploy_gossip.inc();
                }
                MessageKind::BlockGossip => {
                    metrics.in_bytes_block_gossip.inc_by(size);
                    metrics.in_count_block_gossip.inc();
                }
                MessageKind::FinalitySignatureGossip => {
                    metrics.in_bytes_finality_signature_gossip.inc_by(size);
                    metrics.in_count_finality_signature_gossip.inc();
                }
                MessageKind::AddressGossip => {
                    metrics.in_bytes_address_gossip.inc_by(size);
                    metrics.in_count_address_gossip.inc();
                }
                MessageKind::DeployTransfer => {
                    metrics.in_bytes_deploy_transfer.inc_by(size);
                    metrics.in_count_deploy_transfer.inc();
                }
                MessageKind::BlockTransfer => {
                    metrics.in_bytes_block_transfer.inc_by(size);
                    metrics.in_count_block_transfer.inc();
                }
                MessageKind::TrieTransfer => {
                    metrics.in_bytes_trie_transfer.inc_by(size);
                    metrics.in_count_trie_transfer.inc();
                }
                MessageKind::Other => {
                    metrics.in_bytes_other.inc_by(size);
                    metrics.in_count_other.inc();
                }
            }
        } else {
            debug!("not recording metrics, component already shut down");
        }
    }

    /// Creates a set of outgoing metrics that is connected to this set of metrics.
    pub(super) fn create_outgoing_metrics(&self) -> OutgoingMetrics {
        OutgoingMetrics {
            out_state_connecting: self.out_state_connecting.clone(),
            out_state_waiting: self.out_state_waiting.clone(),
            out_state_connected: self.out_state_connected.clone(),
            out_state_blocked: self.out_state_blocked.clone(),
            out_state_loopback: self.out_state_loopback.clone(),
        }
    }

    /// Records that a trie request has been started.
    pub(super) fn record_trie_request_start(this: &Weak<Self>) {
        if let Some(metrics) = this.upgrade() {
            metrics.requests_for_trie_accepted.inc();
        } else {
            debug!("not recording metrics, component already shut down");
        }
    }

    /// Records that a trie request has ended.
    pub(super) fn record_trie_request_end(this: &Weak<Self>) {
        if let Some(metrics) = this.upgrade() {
            metrics.requests_for_trie_finished.inc();
        } else {
            debug!("not recording metrics, component already shut down");
        }
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.broadcast_requests);
        unregister_metric!(self.registry, self.direct_message_requests);
        unregister_metric!(self.registry, self.queued_messages);
        unregister_metric!(self.registry, self.peers);

        unregister_metric!(self.registry, self.out_count_protocol);
        unregister_metric!(self.registry, self.out_count_consensus);
        unregister_metric!(self.registry, self.out_count_deploy_gossip);
        unregister_metric!(self.registry, self.out_count_block_gossip);
        unregister_metric!(self.registry, self.out_count_finality_signature_gossip);
        unregister_metric!(self.registry, self.out_count_address_gossip);
        unregister_metric!(self.registry, self.out_count_deploy_transfer);
        unregister_metric!(self.registry, self.out_count_block_transfer);
        unregister_metric!(self.registry, self.out_count_trie_transfer);
        unregister_metric!(self.registry, self.out_count_other);

        unregister_metric!(self.registry, self.out_bytes_protocol);
        unregister_metric!(self.registry, self.out_bytes_consensus);
        unregister_metric!(self.registry, self.out_bytes_deploy_gossip);
        unregister_metric!(self.registry, self.out_bytes_block_gossip);
        unregister_metric!(self.registry, self.out_bytes_finality_signature_gossip);
        unregister_metric!(self.registry, self.out_bytes_address_gossip);
        unregister_metric!(self.registry, self.out_bytes_deploy_transfer);
        unregister_metric!(self.registry, self.out_bytes_block_transfer);
        unregister_metric!(self.registry, self.out_bytes_trie_transfer);
        unregister_metric!(self.registry, self.out_bytes_other);

        unregister_metric!(self.registry, self.out_state_connecting);
        unregister_metric!(self.registry, self.out_state_waiting);
        unregister_metric!(self.registry, self.out_state_connected);
        unregister_metric!(self.registry, self.out_state_blocked);
        unregister_metric!(self.registry, self.out_state_loopback);

        unregister_metric!(self.registry, self.in_count_protocol);
        unregister_metric!(self.registry, self.in_count_consensus);
        unregister_metric!(self.registry, self.in_count_deploy_gossip);
        unregister_metric!(self.registry, self.in_count_block_gossip);
        unregister_metric!(self.registry, self.in_count_finality_signature_gossip);
        unregister_metric!(self.registry, self.in_count_address_gossip);
        unregister_metric!(self.registry, self.in_count_deploy_transfer);
        unregister_metric!(self.registry, self.in_count_block_transfer);
        unregister_metric!(self.registry, self.in_count_trie_transfer);
        unregister_metric!(self.registry, self.in_count_other);

        unregister_metric!(self.registry, self.in_bytes_protocol);
        unregister_metric!(self.registry, self.in_bytes_consensus);
        unregister_metric!(self.registry, self.in_bytes_deploy_gossip);
        unregister_metric!(self.registry, self.in_bytes_block_gossip);
        unregister_metric!(self.registry, self.in_bytes_finality_signature_gossip);
        unregister_metric!(self.registry, self.in_bytes_address_gossip);
        unregister_metric!(self.registry, self.in_bytes_deploy_transfer);
        unregister_metric!(self.registry, self.in_bytes_block_transfer);
        unregister_metric!(self.registry, self.in_bytes_trie_transfer);
        unregister_metric!(self.registry, self.in_bytes_other);

        unregister_metric!(self.registry, self.requests_for_trie_accepted);
        unregister_metric!(self.registry, self.requests_for_trie_finished);

        unregister_metric!(self.registry, self.accumulated_outgoing_limiter_delay);
        unregister_metric!(self.registry, self.accumulated_incoming_limiter_delay);
    }
}
