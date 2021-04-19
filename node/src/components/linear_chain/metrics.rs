use prometheus::{IntGauge, Registry};

use crate::unregister_metric;

#[derive(Debug)]
pub(super) struct LinearChainMetrics {
    pub(super) block_completion_duration: IntGauge,
    /// Prometheus registry used to publish metrics.
    registry: Registry,
}

impl LinearChainMetrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let block_completion_duration = IntGauge::new(
            "block_completion_duration",
            "duration of time from consensus through execution for a block",
        )?;
        registry.register(Box::new(block_completion_duration.clone()))?;
        Ok(Self {
            block_completion_duration,
            registry: registry.clone(),
        })
    }
}

impl Drop for LinearChainMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.block_completion_duration);
    }
}
