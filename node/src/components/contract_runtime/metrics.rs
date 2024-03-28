use prometheus::{self, Gauge, Histogram, IntGauge, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

/// Value of upper bound of histogram.
const EXPONENTIAL_BUCKET_START: f64 = 0.01;

/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;

/// Bucket count, with the last bucket going to +Inf which will not be included in the results.
/// - start = 0.01, factor = 2.0, count = 10
/// - start * factor ^ count = 0.01 * 2.0 ^ 10 = 10.24
/// - Values above 10.24 (f64 seconds here) will not fall in a bucket that is kept.
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

const RUN_EXECUTE_NAME: &str = "contract_runtime_run_execute";
const RUN_EXECUTE_HELP: &str = "time in seconds to execute but not commit a contract";

const APPLY_EFFECT_NAME: &str = "contract_runtime_apply_commit";
const APPLY_EFFECT_HELP: &str = "time in seconds to commit the execution effects of a contract";

const COMMIT_UPGRADE_NAME: &str = "contract_runtime_commit_upgrade";
const COMMIT_UPGRADE_HELP: &str = "time in seconds to commit an upgrade";

const RUN_QUERY_NAME: &str = "contract_runtime_run_query";
const RUN_QUERY_HELP: &str = "time in seconds to run a query in global state";

const COMMIT_STEP_NAME: &str = "contract_runtime_commit_step";
const COMMIT_STEP_HELP: &str = "time in seconds to commit the step at era end";

const GET_BALANCE_NAME: &str = "contract_runtime_get_balance";
const GET_BALANCE_HELP: &str = "time in seconds to get the balance of a purse from global state";

const GET_ERA_VALIDATORS_NAME: &str = "contract_runtime_get_era_validators";
const GET_ERA_VALIDATORS_HELP: &str =
    "time in seconds to get validators for a given era from global state";

const GET_BIDS_NAME: &str = "contract_runtime_get_bids";
const GET_BIDS_HELP: &str = "time in seconds to get bids from global state";

const PUT_TRIE_NAME: &str = "contract_runtime_put_trie";
const PUT_TRIE_HELP: &str = "time in seconds to put a trie";

const GET_TRIE_NAME: &str = "contract_runtime_get_trie";
const GET_TRIE_HELP: &str = "time in seconds to get a trie";

const EXEC_BLOCK_NAME: &str = "contract_runtime_execute_block";
const EXEC_BLOCK_HELP: &str = "time in seconds to execute all deploys in a block";

const LATEST_COMMIT_STEP_NAME: &str = "contract_runtime_latest_commit_step";
const LATEST_COMMIT_STEP_HELP: &str = "duration in seconds of latest commit step at era end";

const EXEC_QUEUE_SIZE_NAME: &str = "execution_queue_size";
const EXEC_QUEUE_SIZE_HELP: &str =
    "number of blocks that are currently enqueued and waiting for execution";

/// Metrics for the contract runtime component.
#[derive(Debug)]
pub struct Metrics {
    pub(super) run_execute: RegisteredMetric<Histogram>,
    pub(super) apply_effect: RegisteredMetric<Histogram>,
    pub(super) commit_upgrade: RegisteredMetric<Histogram>,
    pub(super) run_query: RegisteredMetric<Histogram>,
    pub(super) commit_step: RegisteredMetric<Histogram>,
    pub(super) get_balance: RegisteredMetric<Histogram>,
    pub(super) get_era_validators: RegisteredMetric<Histogram>,
    pub(super) get_bids: RegisteredMetric<Histogram>,
    pub(super) put_trie: RegisteredMetric<Histogram>,
    pub(super) get_trie: RegisteredMetric<Histogram>,
    pub(super) exec_block: RegisteredMetric<Histogram>,
    pub(super) latest_commit_step: RegisteredMetric<Gauge>,
    pub(super) exec_queue_size: RegisteredMetric<IntGauge>,
}

impl Metrics {
    /// Constructor of metrics which creates and registers metrics objects for use.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let common_buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;

        // Start from 1 millsecond
        // Factor by 2
        // After 10 elements we get to 1s.
        // Anything above that should be a warning signal.
        let tiny_buckets = prometheus::exponential_buckets(0.001, 2.0, 10)?;

        let latest_commit_step =
            registry.new_gauge(LATEST_COMMIT_STEP_NAME, LATEST_COMMIT_STEP_HELP)?;

        let exec_queue_size = registry.new_int_gauge(EXEC_QUEUE_SIZE_NAME, EXEC_QUEUE_SIZE_HELP)?;

        Ok(Metrics {
            run_execute: registry.new_histogram(
                RUN_EXECUTE_NAME,
                RUN_EXECUTE_HELP,
                common_buckets.clone(),
            )?,
            apply_effect: registry.new_histogram(
                APPLY_EFFECT_NAME,
                APPLY_EFFECT_HELP,
                common_buckets.clone(),
            )?,
            run_query: registry.new_histogram(
                RUN_QUERY_NAME,
                RUN_QUERY_HELP,
                common_buckets.clone(),
            )?,
            commit_step: registry.new_histogram(
                COMMIT_STEP_NAME,
                COMMIT_STEP_HELP,
                common_buckets.clone(),
            )?,
            commit_upgrade: registry.new_histogram(
                COMMIT_UPGRADE_NAME,
                COMMIT_UPGRADE_HELP,
                common_buckets.clone(),
            )?,
            get_balance: registry.new_histogram(
                GET_BALANCE_NAME,
                GET_BALANCE_HELP,
                common_buckets.clone(),
            )?,
            get_era_validators: registry.new_histogram(
                GET_ERA_VALIDATORS_NAME,
                GET_ERA_VALIDATORS_HELP,
                common_buckets.clone(),
            )?,
            get_bids: registry.new_histogram(
                GET_BIDS_NAME,
                GET_BIDS_HELP,
                common_buckets.clone(),
            )?,
            get_trie: registry.new_histogram(GET_TRIE_NAME, GET_TRIE_HELP, tiny_buckets.clone())?,
            put_trie: registry.new_histogram(PUT_TRIE_NAME, PUT_TRIE_HELP, tiny_buckets)?,
            exec_block: registry.new_histogram(EXEC_BLOCK_NAME, EXEC_BLOCK_HELP, common_buckets)?,
            latest_commit_step,
            exec_queue_size,
        })
    }
}
