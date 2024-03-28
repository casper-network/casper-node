use prometheus::{Histogram, IntCounter, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

const SYNC_LEAP_DURATION_NAME: &str = "sync_leap_duration_seconds";
const SYNC_LEAP_DURATION_HELP: &str = "duration (in sec) to perform a successful sync leap";

// We use linear buckets to observe the time it takes to do a sync leap.
// Buckets have 1s widths and cover up to 4s durations with this granularity.
const LINEAR_BUCKET_START: f64 = 1.0;
const LINEAR_BUCKET_WIDTH: f64 = 1.0;
const LINEAR_BUCKET_COUNT: usize = 4;

/// Metrics for the sync leap component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Time duration to perform a sync leap.
    pub(super) sync_leap_duration: RegisteredMetric<Histogram>,
    /// Number of successful sync leap responses that were received from peers.
    pub(super) sync_leap_fetched_from_peer: RegisteredMetric<IntCounter>,
    /// Number of requests that were rejected by peers.
    pub(super) sync_leap_rejected_by_peer: RegisteredMetric<IntCounter>,
    /// Number of requests that couldn't be fetched from peers.
    pub(super) sync_leap_cant_fetch: RegisteredMetric<IntCounter>,
}

impl Metrics {
    /// Creates a new instance of the block accumulator metrics, using the given prefix.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let buckets = prometheus::linear_buckets(
            LINEAR_BUCKET_START,
            LINEAR_BUCKET_WIDTH,
            LINEAR_BUCKET_COUNT,
        )?;

        let sync_leap_fetched_from_peer = registry.new_int_counter(
            "sync_leap_fetched_from_peer_total".to_string(),
            "number of successful sync leap responses that were received from peers".to_string(),
        )?;
        let sync_leap_rejected_by_peer = registry.new_int_counter(
            "sync_leap_rejected_by_peer_total".to_string(),
            "number of sync leap requests that were rejected by peers".to_string(),
        )?;
        let sync_leap_cant_fetch = registry.new_int_counter(
            "sync_leap_cant_fetch_total".to_string(),
            "number of sync leap requests that couldn't be fetched from peers".to_string(),
        )?;

        Ok(Metrics {
            sync_leap_duration: registry.new_histogram(
                SYNC_LEAP_DURATION_NAME,
                SYNC_LEAP_DURATION_HELP,
                buckets,
            )?,
            sync_leap_fetched_from_peer,
            sync_leap_rejected_by_peer,
            sync_leap_cant_fetch,
        })
    }
}
