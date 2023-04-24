use prometheus::{IntGauge, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

/// Metrics for the deploy_buffer component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Total number of deploys contained in the deploy buffer.
    pub(super) total_deploys: RegisteredMetric<IntGauge>,
    /// Number of deploys contained in in-flight proposed blocks.
    pub(super) held_deploys: RegisteredMetric<IntGauge>,
    /// Number of deploys that should not be included in future proposals ever again.
    pub(super) dead_deploys: RegisteredMetric<IntGauge>,
}

impl Metrics {
    /// Creates a new instance of the block accumulator metrics, using the given prefix.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let total_deploys = registry.new_int_gauge(
            "deploy_buffer_total_deploys".to_string(),
            "total number of deploys contained in the deploy buffer.".to_string(),
        )?;
        let held_deploys = registry.new_int_gauge(
            "deploy_buffer_held_deploys".to_string(),
            "number of deploys included in in-flight proposed blocks.".to_string(),
        )?;
        let dead_deploys = registry.new_int_gauge(
            "deploy_buffer_dead_deploys".to_string(),
            "number of deploys that should not be included in future proposals.".to_string(),
        )?;

        Ok(Metrics {
            total_deploys,
            held_deploys,
            dead_deploys,
        })
    }
}
