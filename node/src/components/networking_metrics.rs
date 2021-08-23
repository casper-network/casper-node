use std::{error, sync::Weak};

use datasize::DataSize;
use prometheus::{IntCounter, IntGauge, Registry};
use tracing::debug;

use super::small_network::{
    outgoing::{OutgoingManager, OutgoingState},
    MessageKind,
};
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

        registry.register(Box::new(out_state_connecting.clone()))?;
        registry.register(Box::new(out_state_waiting.clone()))?;
        registry.register(Box::new(out_state_connected.clone()))?;
        registry.register(Box::new(out_state_blocked.clone()))?;
        registry.register(Box::new(out_state_loopback.clone()))?;

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
            out_count_other,
            out_bytes_protocol,
            out_bytes_consensus,
            out_bytes_deploy_gossip,
            out_bytes_address_gossip,
            out_bytes_deploy_transfer,
            out_bytes_block_transfer,
            out_bytes_other,
            out_state_connecting,
            out_state_waiting,
            out_state_connected,
            out_state_blocked,
            out_state_loopback,
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
                MessageKind::Other => {
                    metrics.out_bytes_other.inc_by(size);
                    metrics.out_count_other.inc();
                }
            }
        } else {
            debug!("not recording metrics, component already shut down");
        }
    }

    pub(crate) fn update_outgoing<H, E>(&mut self, manager: &OutgoingManager<H, E>)
    where
        H: Clone + DataSize,
        E: DataSize + error::Error,
    {
        let mut connecting = 0;
        let mut waiting = 0;
        let mut connected = 0;
        let mut blocked = 0;
        let mut loopback = 0;

        for (_, outgoing) in manager.iter_outgoing() {
            match outgoing.state {
                OutgoingState::Connecting { .. } => {
                    connecting += 1;
                }
                OutgoingState::Waiting { .. } => {
                    waiting += 1;
                }
                OutgoingState::Connected { .. } => {
                    connected += 1;
                }
                OutgoingState::Blocked { .. } => {
                    blocked += 1;
                }
                OutgoingState::Loopback => {
                    loopback += 1;
                }
            }
        }

        self.out_state_connecting.set(connecting);
        self.out_state_waiting.set(waiting);
        self.out_state_connected.set(connected);
        self.out_state_blocked.set(blocked);
        self.out_state_loopback.set(loopback);
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
        unregister_metric!(self.registry, self.out_count_other);
        unregister_metric!(self.registry, self.out_bytes_protocol);
        unregister_metric!(self.registry, self.out_bytes_consensus);
        unregister_metric!(self.registry, self.out_bytes_deploy_gossip);
        unregister_metric!(self.registry, self.out_bytes_address_gossip);
        unregister_metric!(self.registry, self.out_bytes_deploy_transfer);
        unregister_metric!(self.registry, self.out_bytes_block_transfer);
        unregister_metric!(self.registry, self.out_bytes_other);
        unregister_metric!(self.registry, self.out_state_connecting);
        unregister_metric!(self.registry, self.out_state_waiting);
        unregister_metric!(self.registry, self.out_state_connected);
        unregister_metric!(self.registry, self.out_state_blocked);
        unregister_metric!(self.registry, self.out_state_loopback);
    }
}
