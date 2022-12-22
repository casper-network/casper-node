use prometheus::{self, IntGauge, Registry};

use crate::unregister_metric;

const CHAIN_HEIGHT_NAME: &str = "chain_height";
const CHAIN_HEIGHT_HELP: &str = "highest complete block (DEPRECATED)";

const HIGHEST_AVAILABLE_BLOCK_NAME: &str = "highest_available_block_height";
const HIGHEST_AVAILABLE_BLOCK_HELP: &str =
    "highest height of the available block range (the highest contiguous chain of complete blocks)";

const LOWEST_AVAILABLE_BLOCK_NAME: &str = "lowest_available_block_height";
const LOWEST_AVAILABLE_BLOCK_HELP: &str =
    "lowest height of the available block range (the highest contiguous chain of complete blocks)";

/// Metrics for the storage component.
#[derive(Debug)]
pub struct Metrics {
    // deprecated - replaced by `highest_available_block`
    pub(super) chain_height: IntGauge,
    pub(super) highest_available_block: IntGauge,
    pub(super) lowest_available_block: IntGauge,
    registry: Registry,
}

impl Metrics {
    /// Constructor of metrics which creates and registers metrics objects for use.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let chain_height = IntGauge::new(CHAIN_HEIGHT_NAME, CHAIN_HEIGHT_HELP)?;
        let highest_available_block =
            IntGauge::new(HIGHEST_AVAILABLE_BLOCK_NAME, HIGHEST_AVAILABLE_BLOCK_HELP)?;
        let lowest_available_block =
            IntGauge::new(LOWEST_AVAILABLE_BLOCK_NAME, LOWEST_AVAILABLE_BLOCK_HELP)?;

        registry.register(Box::new(chain_height.clone()))?;
        registry.register(Box::new(highest_available_block.clone()))?;
        registry.register(Box::new(lowest_available_block.clone()))?;

        Ok(Metrics {
            chain_height,
            highest_available_block,
            lowest_available_block,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.chain_height);
        unregister_metric!(self.registry, self.highest_available_block);
        unregister_metric!(self.registry, self.lowest_available_block);
    }
}
