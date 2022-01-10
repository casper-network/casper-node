use prometheus::{IntCounter, IntGauge, Registry};

use crate::unregister_metric;

/// Metrics for the gossiper component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Total number of items received by the gossiper.
    pub(super) items_received: IntCounter,
    /// Total number of gossip requests sent to peers.
    pub(super) times_gossiped: IntCounter,
    /// Number of times the process had to pause due to running out of peers.
    pub(super) times_ran_out_of_peers: IntCounter,
    /// Number of items in the gossip table that are currently being gossiped.
    pub(super) table_items_current: IntGauge,
    /// Number of items in the gossip table that are finished.
    pub(super) table_items_finished: IntGauge,
    /// Reference to the registry for unregistering.
    registry: Registry,
}

impl Metrics {
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
        registry.register(Box::new(table_items_current.clone()))?;
        registry.register(Box::new(table_items_finished.clone()))?;

        Ok(Metrics {
            items_received,
            times_gossiped,
            times_ran_out_of_peers,
            table_items_current,
            table_items_finished,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.items_received);
        unregister_metric!(self.registry, self.times_gossiped);
        unregister_metric!(self.registry, self.times_ran_out_of_peers);
        unregister_metric!(self.registry, self.table_items_current);
        unregister_metric!(self.registry, self.table_items_finished);
    }
}
