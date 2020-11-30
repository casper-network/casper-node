use prometheus::{IntGauge, Registry};

#[derive(Debug, Clone)]
pub struct BlockExecutorMetrics {
    /// The current chain height.
    pub chain_height: IntGauge,
    /// registry component.
    registry: Registry,
}

impl BlockExecutorMetrics {
    pub fn new(registry: Registry) -> Result<Self, prometheus::Error> {
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
        self.registry
            .unregister(Box::new(self.chain_height.clone()))
            .expect("did not expect deregistering chain_height to fail");
    }
}

impl Default for BlockExecutorMetrics {
    fn default() -> Self {
        let registry = Registry::new();
        BlockExecutorMetrics::new(registry).unwrap()
    }
}
