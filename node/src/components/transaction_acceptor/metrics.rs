use prometheus::{Histogram, Registry};

use casper_types::Timestamp;

use crate::{unregister_metric, utils};

const TRANSACTION_ACCEPTED_NAME: &str = "transaction_acceptor_accepted_transaction";
const TRANSACTION_ACCEPTED_HELP: &str =
    "time in seconds to accept a transaction in the transaction acceptor";
const TRANSACTION_REJECTED_NAME: &str = "transaction_acceptor_rejected_transaction";
const TRANSACTION_REJECTED_HELP: &str =
    "time in seconds to reject a transaction in the transaction acceptor";

/// Value of upper bound of the first bucket. In ms.
const EXPONENTIAL_BUCKET_START_MS: f64 = 10.0;

/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;

/// Bucket count, with the last bucket going to +Inf which will not be included in the results.
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

#[derive(Debug)]
pub(super) struct Metrics {
    transaction_accepted: Histogram,
    transaction_rejected: Histogram,
    registry: Registry,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let common_buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START_MS,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;

        Ok(Self {
            transaction_accepted: utils::register_histogram_metric(
                registry,
                TRANSACTION_ACCEPTED_NAME,
                TRANSACTION_ACCEPTED_HELP,
                common_buckets.clone(),
            )?,
            transaction_rejected: utils::register_histogram_metric(
                registry,
                TRANSACTION_REJECTED_NAME,
                TRANSACTION_REJECTED_HELP,
                common_buckets,
            )?,
            registry: registry.clone(),
        })
    }

    pub(super) fn observe_rejected(&self, start: Timestamp) {
        self.transaction_rejected
            .observe(start.elapsed().millis() as f64);
    }

    pub(super) fn observe_accepted(&self, start: Timestamp) {
        self.transaction_accepted
            .observe(start.elapsed().millis() as f64);
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.transaction_accepted);
        unregister_metric!(self.registry, self.transaction_rejected);
    }
}
