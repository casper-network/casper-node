//! Storage metrics.

use prometheus::{Counter, Histogram, Registry};

use crate::{unregister_metric, utils};

/// Value of upper bound of histogram.
const EXPONENTIAL_BUCKET_START: f64 = 1.0;

/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 3.0;

/// Bucket count, with the last bucket going to +Inf which will not be included in the results.
/// - start = 1, factor = 3, count = 10
/// - start * factor ^ count = 1 * 3.0 ^ 10 = 19683millis
/// - Values above 19.683s will not fall in a bucket that is kept.
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

#[derive(Debug)]
pub(crate) struct Metrics {
    pub(crate) sync_task_limiter_in_flight_counter: Counter,
    pub(crate) sync_task_limiter_waiting_millis: Histogram,
    /// Reference to the registry for unregistering.
    registry: Registry,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let common_buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;

        let sync_task_limiter_in_flight_counter = Counter::new(
            "storage_sync_task_limiter_in_flight_count",
            "number of currently awaiting tasks on the semaphore",
        )?;
        registry.register(Box::new(sync_task_limiter_in_flight_counter.clone()))?;

        Ok(Metrics {
            sync_task_limiter_in_flight_counter,
            sync_task_limiter_waiting_millis: utils::register_histogram_metric(
                registry,
                "storage_sync_task_limiter_waiting_millis",
                "time in milliseconds spent waiting on a semaphore",
                common_buckets,
            )?,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.sync_task_limiter_waiting_millis);
        unregister_metric!(self.registry, self.sync_task_limiter_in_flight_counter);
    }
}
