use datasize::DataSize;
use prometheus::{self, IntGauge, Registry};

use crate::unregister_metric;

/// Metrics for the block proposer.
#[derive(DataSize, Debug, Clone)]
pub(super) struct BlockProposerMetrics {
    /// Amount of pending deploys
    #[data_size(skip)]
    pub(super) pending_deploys: IntGauge,
    /// Registry stored to allow deregistration later.
    #[data_size(skip)]
    registry: Registry,
}

impl BlockProposerMetrics {
    /// Creates a new instance of the block proposer metrics.
    pub fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let pending_deploys = IntGauge::new("pending_deploy", "amount of pending deploys")?;
        registry.register(Box::new(pending_deploys.clone()))?;
        Ok(BlockProposerMetrics {
            pending_deploys,
            registry,
        })
    }
}

impl Drop for BlockProposerMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.pending_deploys);
    }
}
