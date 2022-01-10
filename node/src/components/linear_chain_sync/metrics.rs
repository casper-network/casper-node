use std::time::Instant;

use prometheus::{Histogram, Registry};

use crate::{unregister_metric, utils};

const GET_BLOCK_BY_HASH: &str = "linear_chain_sync_get_block_by_hash";
const GET_BLOCK_BY_HASH_HELP: &str =
    "time in seconds to get block by hash during linear chain sync";
const GET_BLOCK_BY_HEIGHT: &str = "linear_chain_sync_get_block_by_height";
const GET_BLOCK_BY_HEIGHT_HELP: &str =
    "time in seconds to get block by height during linear chain sync";
const GET_DEPLOYS: &str = "linear_chain_sync_get_deploys";
const GET_DEPLOYS_HELP: &str = "time in seconds to get deploys during linear chain sync";

/// Value of upper bound of histogram.
const EXPONENTIAL_BUCKET_START: f64 = 0.01;
/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;
/// Bucket count, with last going to +Inf.
const EXPONENTIAL_BUCKET_COUNT: usize = 6;

#[derive(Debug)]
pub(super) struct Metrics {
    get_block_by_hash: Histogram,
    get_block_by_height: Histogram,
    get_deploys: Histogram,
    request_start: Instant,
    registry: Registry,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let common_buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;
        Ok(Metrics {
            get_block_by_hash: utils::register_histogram_metric(
                registry,
                GET_BLOCK_BY_HASH,
                GET_BLOCK_BY_HASH_HELP,
                common_buckets.clone(),
            )?,
            get_block_by_height: utils::register_histogram_metric(
                registry,
                GET_BLOCK_BY_HEIGHT,
                GET_BLOCK_BY_HEIGHT_HELP,
                common_buckets.clone(),
            )?,
            get_deploys: utils::register_histogram_metric(
                registry,
                GET_DEPLOYS,
                GET_DEPLOYS_HELP,
                common_buckets,
            )?,
            request_start: Instant::now(),
            registry: registry.clone(),
        })
    }

    pub(super) fn reset_start_time(&mut self) {
        self.request_start = Instant::now();
    }

    pub(super) fn observe_get_block_by_hash(&mut self) {
        self.get_block_by_hash
            .observe(self.request_start.elapsed().as_secs_f64());
    }

    pub(super) fn observe_get_block_by_height(&mut self) {
        self.get_block_by_height
            .observe(self.request_start.elapsed().as_secs_f64());
    }

    pub(super) fn observe_get_deploys(&mut self) {
        self.get_deploys
            .observe(self.request_start.elapsed().as_secs_f64());
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.get_block_by_hash);
        unregister_metric!(self.registry, self.get_block_by_height);
        unregister_metric!(self.registry, self.get_deploys);
    }
}
