use prometheus::{IntGauge, Registry};

use crate::unregister_metric;

#[derive(Debug)]
pub(super) struct Metrics {
    pub(super) block_completion_duration: IntGauge,
    /// Prometheus registry used to publish metrics.
    registry: Registry,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let block_completion_duration = IntGauge::new(
            "block_completion_duration",
            "time in milliseconds to execute a block, from finalizing it until stored locally",
        )?;
        registry.register(Box::new(block_completion_duration.clone()))?;
        Ok(Self {
            block_completion_duration,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.block_completion_duration);
    }
}
