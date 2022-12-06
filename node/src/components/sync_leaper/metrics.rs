use prometheus::{Histogram, IntCounter, Registry};

use crate::{unregister_metric, utils};

const SYNC_LEAP_DURATION_NAME: &str = "sync_leap_duration";
const SYNC_LEAP_DURATION_HELP: &str = "Time duration (in sec) to perform a successful sync leap.";

// We use linear buckets to observe the time it takes to do a sync leap.
// Buckets have 1s widths and cover up to 4s durations with this granularity.
const LINEAR_BUCKET_START: f64 = 1.0;
const LINEAR_BUCKET_WIDTH: f64 = 1.0;
const LINEAR_BUCKET_COUNT: usize = 4;

/// Metrics for the sync leap component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Time duration to perform a sync leap.
    pub(super) sync_leap_duration: Histogram,
    /// Number of successful sync leap responses that were received from peers.
    pub(super) sync_leap_fetched_from_peer: IntCounter,
    /// Number of requests that were rejected by peers.
    pub(super) sync_leap_rejected_by_peer: IntCounter,
    /// Number of requests that couldn't be fetched from peers.
    pub(super) sync_leap_cant_fetch: IntCounter,

    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the block accumulator metrics, using the given prefix.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let buckets = prometheus::linear_buckets(
            LINEAR_BUCKET_START,
            LINEAR_BUCKET_WIDTH,
            LINEAR_BUCKET_COUNT,
        )?;

        let sync_leap_fetched_from_peer = IntCounter::new(
            "sync_leap_fetched_from_peer".to_string(),
            "Number of successful sync leap responses that were received from peers.".to_string(),
        )?;
        let sync_leap_rejected_by_peer = IntCounter::new(
            "sync_leap_rejected_by_peer".to_string(),
            "Number of requests that were rejected by peers.".to_string(),
        )?;
        let sync_leap_cant_fetch = IntCounter::new(
            "sync_leap_cant_fetch".to_string(),
            "Number of requests that couldn't be fetched from peers.".to_string(),
        )?;

        registry.register(Box::new(sync_leap_fetched_from_peer.clone()))?;
        registry.register(Box::new(sync_leap_rejected_by_peer.clone()))?;
        registry.register(Box::new(sync_leap_cant_fetch.clone()))?;

        Ok(Metrics {
            sync_leap_duration: utils::register_histogram_metric(
                registry,
                SYNC_LEAP_DURATION_NAME,
                SYNC_LEAP_DURATION_HELP,
                buckets,
            )?,
            sync_leap_fetched_from_peer,
            sync_leap_rejected_by_peer,
            sync_leap_cant_fetch,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.sync_leap_duration);
        unregister_metric!(self.registry, self.sync_leap_cant_fetch);
        unregister_metric!(self.registry, self.sync_leap_fetched_from_peer);
        unregister_metric!(self.registry, self.sync_leap_rejected_by_peer);
    }
}
