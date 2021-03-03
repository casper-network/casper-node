use prometheus::{IntCounter, IntGauge, Registry};

/// Metrics for the gossiper component.
#[derive(Debug)]
pub struct GossiperMetrics {
    /// Total number of items received by the gossiper.
    pub(super) items_received: IntCounter,
    /// Total number of gossip requests sent to peers.
    pub(super) times_gossiped: IntCounter,
    /// Number of times the process had to pause due to running out of peers.
    pub(super) times_ran_out_of_peers: IntCounter,
    /// Number of items in the gossip table that are paused.
    pub(super) table_items_paused: IntGauge,
    /// Number of items in the gossip table that are currently being gossiped.
    pub(super) table_items_current: IntGauge,
    /// Number of items in the gossip table that are finished.
    pub(super) table_items_finished: IntGauge,
    /// Reference to the registry for unregistering.
    registry: Registry,
}

impl GossiperMetrics {
    /// Creates a new instance of gossiper metrics, using the given prefix.
    pub fn new(name: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        let items_received = IntCounter::new(
            format!("{}_items_received", name),
            format!("number of items received by the {}", name),
        )?;
        let times_gossiped = IntCounter::new(
            format!("{}_times_gossiped", name),
            format!("number of times the {} sent gossip requests to peers", name),
        )?;
        let times_ran_out_of_peers = IntCounter::new(
            format!("{}_times_ran_out_of_peers", name),
            format!(
                "number of times the {} ran out of peers and had to pause",
                name
            ),
        )?;
        let table_items_paused = IntGauge::new(
            format!("{}_table_items_paused", name),
            format!(
                "number of items in the gossip table of {} in state paused",
                name
            ),
        )?;
        let table_items_current = IntGauge::new(
            format!("{}_table_items_current", name),
            format!(
                "number of items in the gossip table of {} in state current",
                name
            ),
        )?;
        let table_items_finished = IntGauge::new(
            format!("{}_table_items_finished", name),
            format!(
                "number of items in the gossip table of {} in state finished",
                name
            ),
        )?;

        registry.register(Box::new(items_received.clone()))?;
        registry.register(Box::new(times_gossiped.clone()))?;
        registry.register(Box::new(times_ran_out_of_peers.clone()))?;
        registry.register(Box::new(table_items_paused.clone()))?;
        registry.register(Box::new(table_items_current.clone()))?;
        registry.register(Box::new(table_items_finished.clone()))?;

        Ok(GossiperMetrics {
            items_received,
            times_gossiped,
            times_ran_out_of_peers,
            table_items_paused,
            table_items_current,
            table_items_finished,
            registry: registry.clone(),
        })
    }
}

impl Drop for GossiperMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.items_received.clone()))
            .expect("did not expect deregistering items_received to fail");
        self.registry
            .unregister(Box::new(self.times_gossiped.clone()))
            .expect("did not expect deregistering times_gossiped to fail");
        self.registry
            .unregister(Box::new(self.times_ran_out_of_peers.clone()))
            .expect("did not expect deregistering times_ran_out_of_peers to fail");
        self.registry
            .unregister(Box::new(self.table_items_paused.clone()))
            .expect("did not expect deregistering table_items_paused to fail");
        self.registry
            .unregister(Box::new(self.table_items_current.clone()))
            .expect("did not expect deregistering table_items_current to fail");
        self.registry
            .unregister(Box::new(self.table_items_finished.clone()))
            .expect("did not expect deregistering table_items_finished to fail");
    }
}
