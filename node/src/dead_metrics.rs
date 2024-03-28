//! This file contains metrics that have been retired, but are kept around for now to avoid breaking
//! changes to downstream consumers of said metrics.

use prometheus::{IntCounter, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

/// Metrics that are never updated.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) struct DeadMetrics {
    scheduler_queue_network_low_priority_count: RegisteredMetric<IntCounter>,
    scheduler_queue_network_demands_count: RegisteredMetric<IntCounter>,
    accumulated_incoming_limiter_delay: RegisteredMetric<IntCounter>,
    scheduler_queue_network_incoming_count: RegisteredMetric<IntCounter>,
}

impl DeadMetrics {
    /// Creates a new instance of the dead metrics.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let scheduler_queue_network_low_priority_count = registry.new_int_counter(
            "scheduler_queue_network_low_priority_count",
            "retired metric",
        )?;

        let scheduler_queue_network_demands_count =
            registry.new_int_counter("scheduler_queue_network_demands_count", "retired metric")?;

        let accumulated_incoming_limiter_delay =
            registry.new_int_counter("accumulated_incoming_limiter_delay", "retired metric")?;

        let scheduler_queue_network_incoming_count =
            registry.new_int_counter("scheduler_queue_network_incoming_count", "retired metric")?;

        Ok(DeadMetrics {
            scheduler_queue_network_low_priority_count,
            scheduler_queue_network_demands_count,
            accumulated_incoming_limiter_delay,
            scheduler_queue_network_incoming_count,
        })
    }
}
