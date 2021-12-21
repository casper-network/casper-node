use prometheus::{self, Gauge, Histogram, IntGauge, Registry};

use crate::{unregister_metric, utils};

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

const GET_VALIDATOR_WEIGHTS_NAME: &str = "contract_runtime_get_validator_weights";
const GET_VALIDATOR_WEIGHTS_HELP: &str =
    "time in seconds to get validator weights from global state";

const GET_ERA_VALIDATORS_NAME: &str = "contract_runtime_get_era_validators";
const GET_ERA_VALIDATORS_HELP: &str =
    "time in seconds to get validators for a given era from global state";

const GET_BIDS_NAME: &str = "contract_runtime_get_bids";
const GET_BIDS_HELP: &str = "time in seconds to get bids from global state";

const MISSING_TRIE_KEYS_NAME: &str = "contract_runtime_missing_trie_keys";
const MISSING_TRIE_KEYS_HELP: &str = "time in seconds to get missing trie keys";

const PUT_TRIE_NAME: &str = "contract_runtime_put_trie";
const PUT_TRIE_HELP: &str = "time in seconds to put a trie";

const GET_TRIE_NAME: &str = "contract_runtime_get_trie";
const GET_TRIE_HELP: &str = "time in seconds to get a trie";

const CHAIN_HEIGHT_NAME: &str = "chain_height";
const CHAIN_HEIGHT_HELP: &str = "current chain height";

const EXEC_BLOCK_NAME: &str = "contract_runtime_execute_block";
const EXEC_BLOCK_HELP: &str = "time in seconds to execute all deploys in a block";

const LATEST_COMMIT_STEP_NAME: &str = "contract_runtime_latest_commit_step";
const LATEST_COMMIT_STEP_HELP: &str = "duration in seconds of latest commit step at era end";

/// Metrics for the contract runtime component.
#[derive(Debug)]
pub struct Metrics {
    pub(super) run_execute: Histogram,
    pub(super) apply_effect: Histogram,
    pub(super) commit_upgrade: Histogram,
    pub(super) run_query: Histogram,
    pub(super) commit_step: Histogram,
    pub(super) get_balance: Histogram,
    pub(super) get_validator_weights: Histogram,
    pub(super) get_era_validators: Histogram,
    pub(super) get_bids: Histogram,
    pub(super) missing_trie_keys: Histogram,
    pub(super) put_trie: Histogram,
    pub(super) get_trie: Histogram,
    pub(super) chain_height: IntGauge,
    pub(super) exec_block: Histogram,
    pub(super) latest_commit_step: Gauge,
    registry: Registry,
}

impl Metrics {
    /// Constructor of metrics which creates and registers metrics objects for use.
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let common_buckets = prometheus::exponential_buckets(
            EXPONENTIAL_BUCKET_START,
            EXPONENTIAL_BUCKET_FACTOR,
            EXPONENTIAL_BUCKET_COUNT,
        )?;

        let chain_height = IntGauge::new(CHAIN_HEIGHT_NAME, CHAIN_HEIGHT_HELP)?;
        registry.register(Box::new(chain_height.clone()))?;

        let latest_commit_step = Gauge::new(LATEST_COMMIT_STEP_NAME, LATEST_COMMIT_STEP_HELP)?;
        registry.register(Box::new(latest_commit_step.clone()))?;

        Ok(Metrics {
            run_execute: utils::register_histogram_metric(
                registry,
                RUN_EXECUTE_NAME,
                RUN_EXECUTE_HELP,
                common_buckets.clone(),
            )?,
            apply_effect: utils::register_histogram_metric(
                registry,
                APPLY_EFFECT_NAME,
                APPLY_EFFECT_HELP,
                common_buckets.clone(),
            )?,
            run_query: utils::register_histogram_metric(
                registry,
                RUN_QUERY_NAME,
                RUN_QUERY_HELP,
                common_buckets.clone(),
            )?,
            commit_step: utils::register_histogram_metric(
                registry,
                COMMIT_STEP_NAME,
                COMMIT_STEP_HELP,
                common_buckets.clone(),
            )?,
            commit_upgrade: utils::register_histogram_metric(
                registry,
                COMMIT_UPGRADE_NAME,
                COMMIT_UPGRADE_HELP,
                common_buckets.clone(),
            )?,
            get_balance: utils::register_histogram_metric(
                registry,
                GET_BALANCE_NAME,
                GET_BALANCE_HELP,
                common_buckets.clone(),
            )?,
            get_validator_weights: utils::register_histogram_metric(
                registry,
                GET_VALIDATOR_WEIGHTS_NAME,
                GET_VALIDATOR_WEIGHTS_HELP,
                common_buckets.clone(),
            )?,
            get_era_validators: utils::register_histogram_metric(
                registry,
                GET_ERA_VALIDATORS_NAME,
                GET_ERA_VALIDATORS_HELP,
                common_buckets.clone(),
            )?,
            get_bids: utils::register_histogram_metric(
                registry,
                GET_BIDS_NAME,
                GET_BIDS_HELP,
                common_buckets.clone(),
            )?,
            get_trie: utils::register_histogram_metric(
                registry,
                GET_TRIE_NAME,
                GET_TRIE_HELP,
                common_buckets.clone(),
            )?,
            put_trie: utils::register_histogram_metric(
                registry,
                PUT_TRIE_NAME,
                PUT_TRIE_HELP,
                common_buckets.clone(),
            )?,
            missing_trie_keys: utils::register_histogram_metric(
                registry,
                MISSING_TRIE_KEYS_NAME,
                MISSING_TRIE_KEYS_HELP,
                common_buckets.clone(),
            )?,
            chain_height,
            exec_block: utils::register_histogram_metric(
                registry,
                EXEC_BLOCK_NAME,
                EXEC_BLOCK_HELP,
                common_buckets,
            )?,
            latest_commit_step,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.run_execute);
        unregister_metric!(self.registry, self.apply_effect);
        unregister_metric!(self.registry, self.commit_upgrade);
        unregister_metric!(self.registry, self.run_query);
        unregister_metric!(self.registry, self.commit_step);
        unregister_metric!(self.registry, self.get_balance);
        unregister_metric!(self.registry, self.get_validator_weights);
        unregister_metric!(self.registry, self.get_era_validators);
        unregister_metric!(self.registry, self.get_bids);
        unregister_metric!(self.registry, self.missing_trie_keys);
        unregister_metric!(self.registry, self.put_trie);
        unregister_metric!(self.registry, self.get_trie);
        unregister_metric!(self.registry, self.chain_height);
        unregister_metric!(self.registry, self.exec_block);
        unregister_metric!(self.registry, self.latest_commit_step);
    }
}
