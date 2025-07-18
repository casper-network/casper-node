use prometheus::{self, Histogram, IntGauge, Registry};

use crate::{unregister_metric, utils};

const CHAIN_HEIGHT_NAME: &str = "chain_height";
const CHAIN_HEIGHT_HELP: &str = "highest complete block (DEPRECATED)";

const HIGHEST_AVAILABLE_BLOCK_NAME: &str = "highest_available_block_height";
const HIGHEST_AVAILABLE_BLOCK_HELP: &str =
    "highest height of the available block range (the highest contiguous chain of complete blocks)";

const LOWEST_AVAILABLE_BLOCK_NAME: &str = "lowest_available_block_height";
const LOWEST_AVAILABLE_BLOCK_HELP: &str =
    "lowest height of the available block range (the highest contiguous chain of complete blocks)";

const SYNC_LEAP_DURATION_NAME: &str = "storage_sync_leap_duration_seconds";
const SYNC_LEAP_DURATION_HELP: &str = "duration (in sec) to process a sync leap";

// We use exponential buckets to observe the time it takes to synchronize blocks.
// Coverage is ~7.7s with higher resolution in the first buckets.
const EXPONENTIAL_BUCKET_START: f64 = 0.2;
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

/// Metrics for the storage component.
#[derive(Debug)]
pub struct Metrics {
    // deprecated - replaced by `highest_available_block`
    pub(super) chain_height: IntGauge,
    pub(super) highest_available_block: IntGauge,
    pub(super) lowest_available_block: IntGauge,
    pub(super) sync_leap: Histogram,
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

        let sync_leap = utils::register_histogram_metric(
            registry,
            SYNC_LEAP_DURATION_NAME,
            SYNC_LEAP_DURATION_HELP,
            prometheus::exponential_buckets(
                EXPONENTIAL_BUCKET_START,
                EXPONENTIAL_BUCKET_FACTOR,
                EXPONENTIAL_BUCKET_COUNT,
            )?,
        )?;

        Ok(Metrics {
            chain_height,
            highest_available_block,
            lowest_available_block,
            sync_leap,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.chain_height);
        unregister_metric!(self.registry, self.highest_available_block);
        unregister_metric!(self.registry, self.lowest_available_block);
        unregister_metric!(self.registry, self.sync_leap);
    }
}
