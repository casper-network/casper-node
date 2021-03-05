use prometheus::{IntGauge, Registry};

use crate::unregister_metric;

#[derive(Debug, Clone)]
pub(super) struct BlockExecutorMetrics {
    /// The current chain height.
    pub(super) chain_height: IntGauge,
    /// registry component.
    registry: Registry,
}

impl BlockExecutorMetrics {
    pub(super) fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let chain_height = IntGauge::new("chain_height", "current chain height")?;
        registry.register(Box::new(chain_height.clone()))?;
        Ok(BlockExecutorMetrics {
            chain_height,
            registry,
        })
    }
}

impl Drop for BlockExecutorMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.chain_height);
    }
}

impl Default for BlockExecutorMetrics {
    fn default() -> Self {
        let registry = Registry::new();
        BlockExecutorMetrics::new(registry).unwrap()
    }
}
