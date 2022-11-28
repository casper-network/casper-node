use prometheus::{Histogram, Registry};

use crate::{unregister_metric, utils};

const HIST_SYNC_TIME_NAME: &str = "historical_block_sync_time";
const HIST_SYNC_TIME_HELP: &str = "Time duration (in sec) to synchronize a historical block";
const FWD_SYNC_TIME_NAME: &str = "forward_block_sync_time";
const FWD_SYNC_TIME_HELP: &str = "Time duration (in sec) to synchronize a forward block";

// We use linear buckets to observe the time it takes to synchonize blocks.
// Buckets have 50 milisec widths and cover up to 500 milisec durations with this granularity.
const LINEAR_BUCKET_START: f64 = 0.05;
const LINEAR_BUCKET_WIDTH: f64 = 0.05;
const LINEAR_BUCKET_COUNT: usize = 10;

/// Metrics for the block synchronizer component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Time duration for the historical synchronizer to get a block.
    pub(super) historical_block_sync_time: Histogram,
    /// Time duration for the forward synchronizer to get a block.
    pub(super) forward_block_sync_time: Histogram,
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the block synchronizer metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let buckets = prometheus::linear_buckets(
            LINEAR_BUCKET_START,
            LINEAR_BUCKET_WIDTH,
            LINEAR_BUCKET_COUNT,
        )?;

        Ok(Metrics {
            historical_block_sync_time: utils::register_histogram_metric(
                registry,
                HIST_SYNC_TIME_NAME,
                HIST_SYNC_TIME_HELP,
                buckets.clone(),
            )?,
            forward_block_sync_time: utils::register_histogram_metric(
                registry,
                FWD_SYNC_TIME_NAME,
                FWD_SYNC_TIME_HELP,
                buckets,
            )?,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.historical_block_sync_time);
        unregister_metric!(self.registry, self.forward_block_sync_time);
    }
}
