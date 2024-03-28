use prometheus::{Histogram, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

const HIST_SYNC_DURATION_NAME: &str = "historical_block_sync_duration_seconds";
const HIST_SYNC_DURATION_HELP: &str = "duration (in sec) to synchronize a historical block";
const FWD_SYNC_DURATION_NAME: &str = "forward_block_sync_duration_seconds";
const FWD_SYNC_DURATION_HELP: &str = "duration (in sec) to synchronize a forward block";

// We use exponential buckets to observe the time it takes to synchronize blocks.
// Coverage is ~7.7s with higher resolution in the first buckets.
const EXPONENTIAL_BUCKET_START: f64 = 0.05;
const EXPONENTIAL_BUCKET_FACTOR: f64 = 1.75;
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

/// Metrics for the block synchronizer component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Time duration for the historical synchronizer to get a block.
    pub(super) historical_block_sync_duration: RegisteredMetric<Histogram>,
    /// Time duration for the forward synchronizer to get a block.
    pub(super) forward_block_sync_duration: RegisteredMetric<Histogram>,
}

impl Metrics {
    /// Creates a new instance of the block synchronizer metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;

        Ok(Metrics {
            historical_block_sync_duration: registry.new_histogram(
                HIST_SYNC_DURATION_NAME,
                HIST_SYNC_DURATION_HELP,
                buckets.clone(),
            )?,
            forward_block_sync_duration: registry.new_histogram(
                FWD_SYNC_DURATION_NAME,
                FWD_SYNC_DURATION_HELP,
                buckets,
            )?,
        })
    }
}
