use prometheus::{Histogram, Registry};

use casper_types::Timestamp;

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

const DEPLOY_ACCEPTED_NAME: &str = "deploy_acceptor_accepted_deploy";
const DEPLOY_ACCEPTED_HELP: &str = "time in seconds to accept a deploy in the deploy acceptor";
const DEPLOY_REJECTED_NAME: &str = "deploy_acceptor_rejected_deploy";
const DEPLOY_REJECTED_HELP: &str = "time in seconds to reject a deploy in the deploy acceptor";

/// Value of upper bound of the first bucked. In ms.
const EXPONENTIAL_BUCKET_START: f64 = 10.0;

/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;

/// Bucket count, with the last bucket going to +Inf which will not be included in the results.
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

#[derive(Debug)]
pub(super) struct Metrics {
    deploy_accepted: RegisteredMetric<Histogram>,
    deploy_rejected: RegisteredMetric<Histogram>,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let common_buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;

        Ok(Self {
            deploy_accepted: registry.new_histogram(
                DEPLOY_ACCEPTED_NAME,
                DEPLOY_ACCEPTED_HELP,
                common_buckets.clone(),
            )?,
            deploy_rejected: registry.new_histogram(
                DEPLOY_REJECTED_NAME,
                DEPLOY_REJECTED_HELP,
                common_buckets,
            )?,
        })
    }

    pub(super) fn observe_rejected(&self, start: Timestamp) {
        self.deploy_rejected
            .observe(start.elapsed().millis() as f64);
    }

    pub(super) fn observe_accepted(&self, start: Timestamp) {
        self.deploy_accepted
            .observe(start.elapsed().millis() as f64);
    }
}
