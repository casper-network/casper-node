use prometheus::{IntCounter, IntGauge, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

/// Metrics for the gossiper component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Total number of items received by the gossiper.
    pub(super) items_received: RegisteredMetric<IntCounter>,
    /// Total number of gossip requests sent to peers.
    pub(super) times_gossiped: RegisteredMetric<IntCounter>,
    /// Number of times the process had to pause due to running out of peers.
    pub(super) times_ran_out_of_peers: RegisteredMetric<IntCounter>,
    /// Number of items in the gossip table that are currently being gossiped.
    pub(super) table_items_current: RegisteredMetric<IntGauge>,
    /// Number of items in the gossip table that are finished.
    pub(super) table_items_finished: RegisteredMetric<IntGauge>,
}

impl Metrics {
    /// Creates a new instance of gossiper metrics, using the given prefix.
    pub fn new(name: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        let items_received = registry.new_int_counter(
            format!("{}_items_received", name),
            format!("number of items received by the {}", name),
        )?;
        let times_gossiped = registry.new_int_counter(
            format!("{}_times_gossiped", name),
            format!("number of times the {} sent gossip requests to peers", name),
        )?;
        let times_ran_out_of_peers = registry.new_int_counter(
            format!("{}_times_ran_out_of_peers", name),
            format!(
                "number of times the {} ran out of peers and had to pause",
                name
            ),
        )?;
        let table_items_current = registry.new_int_gauge(
            format!("{}_table_items_current", name),
            format!(
                "number of items in the gossip table of {} in state current",
                name
            ),
        )?;
        let table_items_finished = registry.new_int_gauge(
            format!("{}_table_items_finished", name),
            format!(
                "number of items in the gossip table of {} in state finished",
                name
            ),
        )?;

        Ok(Metrics {
            items_received,
            times_gossiped,
            times_ran_out_of_peers,
            table_items_current,
            table_items_finished,
        })
    }
}
