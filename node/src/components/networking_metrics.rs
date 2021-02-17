use prometheus::{IntCounter, IntGauge, Registry};

/// Network-type agnostic networking metrics.
pub(crate) struct NetworkingMetrics {
    /// How often a request was made by a component to broadcast.
    pub(crate) broadcast_requests: IntCounter,
    /// How often a request to send a message directly to a peer was made.
    pub(crate) direct_message_requests: IntCounter,
    /// Current number of open connections.
    pub(crate) open_connections: IntGauge,
    /// Number of messages still waiting to be sent out (broadcast and direct).
    pub(crate) queued_messages: IntGauge,
    /// Number of connected peers.
    pub(crate) peers: IntGauge,

    // Potentially temporary metrics, not supported by all networking components:
    /// Number of do-nothing futures that have not finished executing for read requests.
    pub(crate) read_futures_in_flight: prometheus::Gauge,
    /// Number of do-nothing futures created total (read).
    pub(crate) read_futures_total: prometheus::Gauge,
    /// Number of do-nothing futures that have not finished executing for write reponses.
    pub(crate) write_futures_in_flight: prometheus::Gauge,
    /// Number of do-nothing futures created total (write).
    pub(crate) write_futures_total: prometheus::Gauge,

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
            "number of requests to send a message directly to a peer",
        )?;
        let open_connections =
            IntGauge::new("net_open_connections", "number of established connections")?;
        let queued_messages = IntGauge::new(
            "net_queued_direct_messages",
            "number of messages waiting to be sent out",
        )?;
        let peers = IntGauge::new("peers", "Number of connected peers.")?;

        let read_futures_in_flight = prometheus::Gauge::new(
            "owm_read_futures_in_flight",
            "number of do-nothing futures in flight created by `Codec::read_response`",
        )?;
        let read_futures_total = prometheus::Gauge::new(
            "owm_read_futures_total",
            "number of do-nothing futures total created by `Codec::read_response`",
        )?;
        let write_futures_in_flight = prometheus::Gauge::new(
            "owm_write_futures_in_flight",
            "number of do-nothing futures in flight created by `Codec::write_response`",
        )?;
        let write_futures_total = prometheus::Gauge::new(
            "owm_write_futures_total",
            "number of do-nothing futures total created by `Codec::write_response`",
        )?;

        registry.register(Box::new(broadcast_requests.clone()))?;
        registry.register(Box::new(direct_message_requests.clone()))?;
        registry.register(Box::new(open_connections.clone()))?;
        registry.register(Box::new(queued_messages.clone()))?;
        registry.register(Box::new(peers.clone()))?;

        registry.register(Box::new(read_futures_in_flight.clone()))?;
        registry.register(Box::new(read_futures_total.clone()))?;
        registry.register(Box::new(write_futures_in_flight.clone()))?;
        registry.register(Box::new(write_futures_total.clone()))?;

        Ok(NetworkingMetrics {
            broadcast_requests,
            direct_message_requests,
            open_connections,
            queued_messages,
            peers,
            read_futures_in_flight,
            read_futures_total,
            write_futures_in_flight,
            write_futures_total,
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

        self.registry
            .unregister(Box::new(self.read_futures_in_flight.clone()))
            .expect("did not expect deregistering in_flight_read_futures to fail");
        self.registry
            .unregister(Box::new(self.read_futures_total.clone()))
            .expect("did not expect deregistering total_read_futures to fail");
        self.registry
            .unregister(Box::new(self.write_futures_in_flight.clone()))
            .expect("did not expect deregistering in_flight_write_futures to fail");
        self.registry
            .unregister(Box::new(self.write_futures_total.clone()))
            .expect("did not expect deregistering total_write_futures to fail");
    }
}
