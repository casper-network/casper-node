use prometheus::{IntCounter, IntGauge, Registry};

/// Network-type agnostic networking metrics.
pub(crate) struct NetworkingMetrics {
    /// How often a request was made a component to broadcast.
    pub(crate) broadcast_requests: IntCounter,
    /// How often a request to send a message directly to a peer was made.
    pub(crate) direct_message_requests: IntCounter,
    /// Number of messages received via gossip.
    pub(crate) incoming_gossip_messages: IntCounter,
    /// Number of messages received directly.
    pub(crate) incoming_direct_messages: IntCounter,
    /// Current number of open connections.
    pub(crate) open_connnections: IntGauge,
    /// Number of messages still waiting to be broadcast.
    pub(crate) queued_broadcast_messages: IntGauge,
    /// Number of messages still waiting to be sent out.
    pub(crate) queued_direct_messages: IntGauge,

    /// Registry instance,
    registry: Registry,
}

impl NetworkingMetrics {
    /// Creates a new instance of networking metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let broadcast_requests =
            IntCounter::new("net_broadcast_requests", "number of broadcasting requests")?;
        let direct_message_requests = IntCounter::new(
            "net_direct_message_requests",
            "How often a request to send a message directly to a peer was made.",
        )?;
        let incoming_gossip_messages = IntCounter::new(
            "net_incoming_gossip_messages",
            "Number of messages received via gossip.",
        )?;
        let incoming_direct_messages = IntCounter::new(
            "net_incoming_direct_messages",
            "Number of messages received directly.",
        )?;
        let open_connnections = IntGauge::new(
            "net_open_connnections",
            "Current number of open connections.",
        )?;
        let queued_broadcast_messages = IntGauge::new(
            "net_queued_broadcast_messages",
            "Number of messages still waiting to be broadcast.",
        )?;
        let queued_direct_messages = IntGauge::new(
            "net_queued_direct_messages",
            "Number of messages still waiting to be sent out.",
        )?;

        registry.register(Box::new(broadcast_requests.clone()))?;
        registry.register(Box::new(direct_message_requests.clone()))?;
        registry.register(Box::new(incoming_gossip_messages.clone()))?;
        registry.register(Box::new(incoming_direct_messages.clone()))?;
        registry.register(Box::new(open_connnections.clone()))?;
        registry.register(Box::new(queued_broadcast_messages.clone()))?;
        registry.register(Box::new(queued_direct_messages.clone()))?;

        Ok(NetworkingMetrics {
            broadcast_requests,
            direct_message_requests,
            incoming_gossip_messages,
            incoming_direct_messages,
            open_connnections,
            queued_broadcast_messages,
            queued_direct_messages,
            registry: registry.clone(),
        })
    }
}

impl Drop for NetworkingMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.broadcast_requests.clone()))
            .expect("did not expect deregistering broadcast_requests to fail");
        self.registry
            .unregister(Box::new(self.direct_message_requests.clone()))
            .expect("did not expect deregistering direct_message_requests to fail");
        self.registry
            .unregister(Box::new(self.incoming_gossip_messages.clone()))
            .expect("did not expect deregistering incoming_gossip_messages to fail");
        self.registry
            .unregister(Box::new(self.incoming_direct_messages.clone()))
            .expect("did not expect deregistering incoming_direct_messages to fail");
        self.registry
            .unregister(Box::new(self.open_connnections.clone()))
            .expect("did not expect deregistering open_connnections to fail");
        self.registry
            .unregister(Box::new(self.queued_broadcast_messages.clone()))
            .expect("did not expect deregistering queued_broadcast_messages to fail");
        self.registry
            .unregister(Box::new(self.queued_direct_messages.clone()))
            .expect("did not expect deregistering queued_direct_messages to fail");
    }
}
