use prometheus::{IntCounter, IntGauge, Registry};

/// Network-type agnostic networking metrics.
pub(crate) struct NetworkingMetrics {
    /// How often a request was made a component to broadcast.
    pub(crate) broadcast_requests: IntCounter,
    /// How often a request to send a message directly to a peer was made.
    pub(crate) direct_message_requests: IntCounter,
    /// Current number of open connections.
    pub(crate) open_connections: IntGauge,
    /// Number of messages still waiting to be sent out (broadcast and direct).
    pub(crate) queued_messages: IntGauge,
    /// Number of peers.
    pub(crate) peers: IntGauge,

    /// Registry instance.
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
        let open_connections = IntGauge::new(
            "net_open_connnections",
            "Current number of open connections.",
        )?;
        let queued_messages = IntGauge::new(
            "net_queued_direct_messages",
            "Number of messages still waiting to be sent out.",
        )?;
        let peers = IntGauge::new("peers", "Number of connected peers.")?;

        registry.register(Box::new(broadcast_requests.clone()))?;
        registry.register(Box::new(direct_message_requests.clone()))?;
        registry.register(Box::new(open_connections.clone()))?;
        registry.register(Box::new(queued_messages.clone()))?;
        registry.register(Box::new(peers.clone()))?;

        Ok(NetworkingMetrics {
            broadcast_requests,
            direct_message_requests,
            open_connections,
            queued_messages,
            peers,
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
            .unregister(Box::new(self.open_connections.clone()))
            .expect("did not expect deregistering open_connnections to fail");
        self.registry
            .unregister(Box::new(self.queued_messages.clone()))
            .expect("did not expect deregistering queued_direct_messages to fail");
        self.registry
            .unregister(Box::new(self.peers.clone()))
            .expect("did not expect deregistering peers to fail");
    }
}
