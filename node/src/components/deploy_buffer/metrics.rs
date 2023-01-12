use prometheus::{IntGauge, Registry};

use crate::unregister_metric;

/// Metrics for the deploy_buffer component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Total number of deploys contained in the deploy buffer.
    pub(super) total_deploys: IntGauge,
    /// Number of deploys contained in in-flight proposed blocks.
    pub(super) held_deploys: IntGauge,
    /// Number of deploys that should not be included in future proposals ever again.
    pub(super) dead_deploys: IntGauge,
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the block accumulator metrics, using the given prefix.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let total_deploys = IntGauge::new(
            "deploy_buffer_total_deploys".to_string(),
            "total number of deploys contained in the deploy buffer.".to_string(),
        )?;
        let held_deploys = IntGauge::new(
            "deploy_buffer_held_deploys".to_string(),
            "number of deploys included in in-flight proposed blocks.".to_string(),
        )?;
        let dead_deploys = IntGauge::new(
            "deploy_buffer_dead_deploys".to_string(),
            "number of deploys that should not be included in future proposals.".to_string(),
        )?;

        registry.register(Box::new(total_deploys.clone()))?;
        registry.register(Box::new(held_deploys.clone()))?;
        registry.register(Box::new(dead_deploys.clone()))?;

        Ok(Metrics {
            total_deploys,
            held_deploys,
            dead_deploys,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.total_deploys);
        unregister_metric!(self.registry, self.held_deploys);
        unregister_metric!(self.registry, self.dead_deploys);
    }
}
