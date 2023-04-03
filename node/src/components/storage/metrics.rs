use prometheus::{self, IntGauge, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

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
    pub(super) chain_height: RegisteredMetric<IntGauge>,
    pub(super) highest_available_block: RegisteredMetric<IntGauge>,
    pub(super) lowest_available_block: RegisteredMetric<IntGauge>,
}

impl Metrics {
    /// Constructor of metrics which creates and registers metrics objects for use.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let chain_height = registry.new_int_gauge(CHAIN_HEIGHT_NAME, CHAIN_HEIGHT_HELP)?;
        let highest_available_block =
            registry.new_int_gauge(HIGHEST_AVAILABLE_BLOCK_NAME, HIGHEST_AVAILABLE_BLOCK_HELP)?;
        let lowest_available_block =
            registry.new_int_gauge(LOWEST_AVAILABLE_BLOCK_NAME, LOWEST_AVAILABLE_BLOCK_HELP)?;

        Ok(Metrics {
            chain_height,
            highest_available_block,
            lowest_available_block,
        })
    }
}
