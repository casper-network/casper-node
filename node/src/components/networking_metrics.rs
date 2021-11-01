use std::sync::Weak;

use prometheus::{IntCounter, IntGauge, Registry};
use tracing::debug;

use super::small_network::MessageKind;
use crate::unregister_metric;

/// Network-type agnostic networking metrics.
#[derive(Debug)]
pub(super) struct NetworkingMetrics {
    /// How often a request was made by a component to broadcast.
    pub(super) broadcast_requests: IntCounter,
    /// How often a request to send a message directly to a peer was made.
    pub(super) direct_message_requests: IntCounter,
    /// Current number of open connections.
    pub(super) open_connections: IntGauge,
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

    /// Registry instance.
    registry: Registry,
}

impl NetworkingMetrics {
    /// Creates a new instance of networking metrics.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let broadcast_requests =
            IntCounter::new("net_broadcast_requests", "number of broadcasting requests")?;
        let direct_message_requests = IntCounter::new(
            "net_direct_message_requests",
            "number of requests to send a message directly to a peer",
        )?;
        let open_connections =
            IntGauge::new("net_open_connections", "number of established connections")?;
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
            "net_out_count_block_transfer",
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
            "net_out_bytes_block_transfer",
            "volume in bytes of outgoing messages with trie payloads",
        )?;
        let out_bytes_other = IntCounter::new(
            "net_out_bytes_other",
            "volume in bytes of outgoing messages with other payload",
        )?;

        registry.register(Box::new(broadcast_requests.clone()))?;
        registry.register(Box::new(direct_message_requests.clone()))?;
        registry.register(Box::new(open_connections.clone()))?;
        registry.register(Box::new(queued_messages.clone()))?;
        registry.register(Box::new(peers.clone()))?;

        registry.register(Box::new(out_count_protocol.clone()))?;
        registry.register(Box::new(out_count_consensus.clone()))?;
        registry.register(Box::new(out_count_deploy_gossip.clone()))?;
        registry.register(Box::new(out_count_address_gossip.clone()))?;
        registry.register(Box::new(out_count_deploy_transfer.clone()))?;
        registry.register(Box::new(out_count_block_transfer.clone()))?;
        registry.register(Box::new(out_count_other.clone()))?;

        registry.register(Box::new(out_bytes_protocol.clone()))?;
        registry.register(Box::new(out_bytes_consensus.clone()))?;
        registry.register(Box::new(out_bytes_deploy_gossip.clone()))?;
        registry.register(Box::new(out_bytes_address_gossip.clone()))?;
        registry.register(Box::new(out_bytes_deploy_transfer.clone()))?;
        registry.register(Box::new(out_bytes_block_transfer.clone()))?;
        registry.register(Box::new(out_bytes_other.clone()))?;

        Ok(NetworkingMetrics {
            broadcast_requests,
            direct_message_requests,
            open_connections,
            queued_messages,
            peers,
            out_count_protocol,
            out_count_consensus,
            out_count_deploy_gossip,
            out_count_address_gossip,
            out_count_deploy_transfer,
            out_count_block_transfer,
            out_count_trie_transfer,
            out_count_other,
            out_bytes_protocol,
            out_bytes_consensus,
            out_bytes_deploy_gossip,
            out_bytes_address_gossip,
            out_bytes_deploy_transfer,
            out_bytes_block_transfer,
            out_bytes_trie_transfer,
            out_bytes_other,
            registry: registry.clone(),
        })
    }

    /// Records an outgoing payload.
    pub(crate) fn record_payload_out(this: &mut Weak<Self>, kind: MessageKind, size: u64) {
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
}

impl Drop for NetworkingMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.broadcast_requests);
        unregister_metric!(self.registry, self.direct_message_requests);
        unregister_metric!(self.registry, self.open_connections);
        unregister_metric!(self.registry, self.queued_messages);
        unregister_metric!(self.registry, self.peers);

        unregister_metric!(self.registry, self.out_count_protocol);
        unregister_metric!(self.registry, self.out_count_consensus);
        unregister_metric!(self.registry, self.out_count_deploy_gossip);
        unregister_metric!(self.registry, self.out_count_address_gossip);
        unregister_metric!(self.registry, self.out_count_deploy_transfer);
        unregister_metric!(self.registry, self.out_count_block_transfer);
        unregister_metric!(self.registry, self.out_count_trie_transfer);
        unregister_metric!(self.registry, self.out_count_other);
        unregister_metric!(self.registry, self.out_bytes_protocol);
        unregister_metric!(self.registry, self.out_bytes_consensus);
        unregister_metric!(self.registry, self.out_bytes_deploy_gossip);
        unregister_metric!(self.registry, self.out_bytes_address_gossip);
        unregister_metric!(self.registry, self.out_bytes_deploy_transfer);
        unregister_metric!(self.registry, self.out_bytes_block_transfer);
        unregister_metric!(self.registry, self.out_bytes_trie_transfer);
        unregister_metric!(self.registry, self.out_bytes_other);
    }
}
