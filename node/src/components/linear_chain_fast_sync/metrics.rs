use std::time::Instant;

use prometheus::{Histogram, HistogramOpts, Registry};

#[derive(Debug)]
pub struct LinearChainSyncMetrics {
    get_block_by_hash: Histogram,
    get_block_by_height: Histogram,
    get_deploys: Histogram,
    request_start: Instant,
}

const GET_BLOCK_BY_HASH: &str = "linear_chain_sync_get_block_by_hash";
const GET_BLOCK_BY_HASH_HELP: &str = "histogram of linear_chain_sync get_block_by_hash request";
const GET_BLOCK_BY_HEIGHT: &str = "linear_chain_sync_get_block_by_height";
const GET_BLOCK_BY_HEIGHT_HELP: &str = "histogram of linear_chain_sync get_block_by_height request";
const GET_DEPLOYS: &str = "linear_chain_sync_get_deploys";
const GET_DEPLOYS_HELP: &str = "histogram of linear_chain_sync get_deploys request";

/// Value of upper bound of histogram.
const EXPONENTIAL_BUCKET_START: f64 = 0.01;
/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;
/// Bucket count, with last going to +Inf.
const EXPONENTIAL_BUCKET_COUNT: usize = 6;

/// Create prometheus Histogram and register.
fn register_histogram_metric(
    registry: &Registry,
    metric_name: &str,
    metric_help: &str,
) -> Result<Histogram, prometheus::Error> {
    let common_buckets = prometheus::exponential_buckets(
        EXPONENTIAL_BUCKET_START,
        EXPONENTIAL_BUCKET_FACTOR,
        EXPONENTIAL_BUCKET_COUNT,
    )?;
    let histogram_opts = HistogramOpts::new(metric_name, metric_help).buckets(common_buckets);
    let histogram = Histogram::with_opts(histogram_opts)?;
    registry.register(Box::new(histogram.clone()))?;
    Ok(histogram)
}

impl LinearChainSyncMetrics {
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(LinearChainSyncMetrics {
            get_block_by_hash: register_histogram_metric(
                registry,
                GET_BLOCK_BY_HASH,
                GET_BLOCK_BY_HASH_HELP,
            )?,
            get_block_by_height: register_histogram_metric(
                registry,
                GET_BLOCK_BY_HEIGHT,
                GET_BLOCK_BY_HEIGHT_HELP,
            )?,
            get_deploys: register_histogram_metric(registry, GET_DEPLOYS, GET_DEPLOYS_HELP)?,
            request_start: Instant::now(),
        })
    }

    pub fn reset_start_time(&mut self) {
        self.request_start = Instant::now();
    }

    pub fn observe_get_block_by_hash(&mut self) {
        self.get_block_by_hash
            .observe(self.request_start.elapsed().as_secs_f64());
    }

    pub fn observe_get_block_by_height(&mut self) {
        self.get_block_by_height
            .observe(self.request_start.elapsed().as_secs_f64());
    }

    pub fn observe_get_deploys(&mut self) {
        self.get_deploys
            .observe(self.request_start.elapsed().as_secs_f64());
    }
}
