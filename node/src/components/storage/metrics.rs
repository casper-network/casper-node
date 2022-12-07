use prometheus::{self, IntGauge, Registry};

use crate::unregister_metric;

const CHAIN_HEIGHT_NAME: &str = "chain_height";
const CHAIN_HEIGHT_HELP: &str = "highest complete block";

const LOW_SEQ_BLOCK_HEIGHT_NAME: &str = "chain_low_seq_block_height";
const LOW_SEQ_BLOCK_HEIGHT_HELP: &str =
    "lowest of available block range (the highest contiguous chain of complete blocks)";

/// Metrics for the storage component.
#[derive(Debug)]
pub struct Metrics {
    pub(super) chain_height: IntGauge,
    pub(super) chain_low_seq_block_height: IntGauge,
    registry: Registry,
}

impl Metrics {
    /// Constructor of metrics which creates and registers metrics objects for use.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let chain_height = IntGauge::new(CHAIN_HEIGHT_NAME, CHAIN_HEIGHT_HELP)?;
        let chain_low_seq_block_height =
            IntGauge::new(LOW_SEQ_BLOCK_HEIGHT_NAME, LOW_SEQ_BLOCK_HEIGHT_HELP)?;
        registry.register(Box::new(chain_height.clone()))?;
        registry.register(Box::new(chain_low_seq_block_height.clone()))?;

        Ok(Metrics {
            chain_height,
            chain_low_seq_block_height,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.chain_height);
        unregister_metric!(self.registry, self.chain_low_seq_block_height);
    }
}
