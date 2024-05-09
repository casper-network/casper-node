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

const EXEC_WASM_V1_NAME: &str = "contract_runtime_exec_wasm_v1";
const EXEC_WASM_V1_HELP: &str = "time in seconds to execute wasm using the v1 exec engine";

const EXEC_BLOCK_PRE_PROCESSING_NAME: &str = "contract_runtime_exec_block_pre_proc";
const EXEC_BLOCK_PRE_PROCESSING_HELP: &str =
    "processing time in seconds before any transactions have processed";

const EXEC_BLOCK_POST_PROCESSING_NAME: &str = "contract_runtime_exec_block_post_proc";
const EXEC_BLOCK_POST_PROCESSING_HELP: &str =
    "processing time in seconds after all transactions have processed";

const EXEC_BLOCK_STEP_PROCESSING_NAME: &str = "contract_runtime_exec_block_step_proc";
const EXEC_BLOCK_STEP_PROCESSING_HELP: &str = "processing time in seconds of the end of era step";

const EXEC_BLOCK_TOTAL_NAME: &str = "contract_runtime_exec_block_total_proc";
const EXEC_BLOCK_TOTAL_HELP: &str =
    "processing time in seconds for block execution (total elapsed)";

const COMMIT_GENESIS_NAME: &str = "contract_runtime_commit_genesis";
const COMMIT_GENESIS_HELP: &str = "time in seconds to commit an genesis";

const COMMIT_UPGRADE_NAME: &str = "contract_runtime_commit_upgrade";
const COMMIT_UPGRADE_HELP: &str = "time in seconds to commit an upgrade";

const RUN_QUERY_NAME: &str = "contract_runtime_run_query";
const RUN_QUERY_HELP: &str = "time in seconds to run a query in global state";

const RUN_QUERY_BY_PREFIX_NAME: &str = "contract_runtime_run_query_by_prefix";
const RUN_QUERY_BY_PREFIX_HELP: &str = "time in seconds to run a query by prefix in global state";

const COMMIT_STEP_NAME: &str = "contract_runtime_commit_step";
const COMMIT_STEP_HELP: &str = "time in seconds to commit the step at era end";

const GET_BALANCE_NAME: &str = "contract_runtime_get_balance";
const GET_BALANCE_HELP: &str = "time in seconds to get the balance of a purse from global state";

const GET_TOTAL_SUPPLY_NAME: &str = "contract_runtime_get_total_supply";
const GET_TOTAL_SUPPLY_HELP: &str = "time in seconds to get the total supply from global state";

const GET_ROUND_SEIGNIORAGE_RATE_NAME: &str = "contract_runtime_get_round_seigniorage_rate";
const GET_ROUND_SEIGNIORAGE_RATE_HELP: &str =
    "time in seconds to get the round seigniorage rate from global state";

const GET_ERA_VALIDATORS_NAME: &str = "contract_runtime_get_era_validators";
const GET_ERA_VALIDATORS_HELP: &str =
    "time in seconds to get validators for a given era from global state";

const GET_ALL_VALUES_NAME: &str = "contract_runtime_get_all_values";
const GET_ALL_VALUES_NAME_HELP: &str =
    "time in seconds to get all values under a give key from global state";

const EXECUTION_RESULTS_CHECKSUM_NAME: &str = "contract_runtime_execution_results_checksum";
const EXECUTION_RESULTS_CHECKSUM_HELP: &str = "contract_runtime_execution_results_checksum";

const ADDRESSABLE_ENTITY_NAME: &str = "contract_runtime_addressable_entity";
const ADDRESSABLE_ENTITY_HELP: &str = "contract_runtime_addressable_entity";

const ENTRY_POINT_NAME: &str = "contract_runtime_entry_point";
const ENTRY_POINT_HELP: &str = "contract_runtime_entry_point";

const PUT_TRIE_NAME: &str = "contract_runtime_put_trie";
const PUT_TRIE_HELP: &str = "time in seconds to put a trie";

const GET_TRIE_NAME: &str = "contract_runtime_get_trie";
const GET_TRIE_HELP: &str = "time in seconds to get a trie";

const EXEC_BLOCK_TNX_PROCESSING_NAME: &str = "contract_runtime_execute_block";
const EXEC_BLOCK_TNX_PROCESSING_HELP: &str = "time in seconds to execute all deploys in a block";

const LATEST_COMMIT_STEP_NAME: &str = "contract_runtime_latest_commit_step";
const LATEST_COMMIT_STEP_HELP: &str = "duration in seconds of latest commit step at era end";

const EXEC_QUEUE_SIZE_NAME: &str = "execution_queue_size";
const EXEC_QUEUE_SIZE_HELP: &str =
    "number of blocks that are currently enqueued and waiting for execution";

/// Metrics for the contract runtime component.
#[derive(Debug)]
pub struct Metrics {
    pub(super) exec_block_pre_processing: Histogram,
    // elapsed before tnx processing
    pub(super) exec_block_tnx_processing: Histogram,
    // tnx processing elapsed
    pub(super) exec_wasm_v1: Histogram,
    // ee_v1 execution elapsed
    pub(super) exec_block_step_processing: Histogram,
    // step processing elapsed
    pub(super) exec_block_post_processing: Histogram,
    // elapsed after tnx processing
    pub(super) exec_block_total: Histogram,
    // total elapsed
    pub(super) commit_genesis: Histogram,
    pub(super) commit_upgrade: Histogram,
    pub(super) run_query: Histogram,
    pub(super) run_query_by_prefix: Histogram,
    pub(super) commit_step: Histogram,
    pub(super) get_balance: Histogram,
    pub(super) get_total_supply: Histogram,
    pub(super) get_round_seigniorage_rate: Histogram,
    pub(super) get_era_validators: Histogram,
    pub(super) get_all_values: Histogram,
    pub(super) execution_results_checksum: Histogram,
    pub(super) addressable_entity: Histogram,
    pub(super) entry_points: Histogram,
    pub(super) put_trie: Histogram,
    pub(super) get_trie: Histogram,
    pub(super) latest_commit_step: Gauge,
    pub(super) exec_queue_size: IntGauge,
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

        // Start from 1 millsecond
        // Factor by 2
        // After 10 elements we get to 1s.
        // Anything above that should be a warning signal.
        let tiny_buckets = prometheus::exponential_buckets(0.001, 2.0, 10)?;

        let latest_commit_step = Gauge::new(LATEST_COMMIT_STEP_NAME, LATEST_COMMIT_STEP_HELP)?;
        registry.register(Box::new(latest_commit_step.clone()))?;

        let exec_queue_size = IntGauge::new(EXEC_QUEUE_SIZE_NAME, EXEC_QUEUE_SIZE_HELP)?;
        registry.register(Box::new(exec_queue_size.clone()))?;

        Ok(Metrics {
            exec_block_pre_processing: utils::register_histogram_metric(
                registry,
                EXEC_BLOCK_PRE_PROCESSING_NAME,
                EXEC_BLOCK_PRE_PROCESSING_HELP,
                common_buckets.clone(),
            )?,
            exec_block_tnx_processing: utils::register_histogram_metric(
                registry,
                EXEC_BLOCK_TNX_PROCESSING_NAME,
                EXEC_BLOCK_TNX_PROCESSING_HELP,
                common_buckets.clone(),
            )?,
            exec_wasm_v1: utils::register_histogram_metric(
                registry,
                EXEC_WASM_V1_NAME,
                EXEC_WASM_V1_HELP,
                common_buckets.clone(),
            )?,
            exec_block_post_processing: utils::register_histogram_metric(
                registry,
                EXEC_BLOCK_POST_PROCESSING_NAME,
                EXEC_BLOCK_POST_PROCESSING_HELP,
                common_buckets.clone(),
            )?,
            exec_block_step_processing: utils::register_histogram_metric(
                registry,
                EXEC_BLOCK_STEP_PROCESSING_NAME,
                EXEC_BLOCK_STEP_PROCESSING_HELP,
                common_buckets.clone(),
            )?,
            exec_block_total: utils::register_histogram_metric(
                registry,
                EXEC_BLOCK_TOTAL_NAME,
                EXEC_BLOCK_TOTAL_HELP,
                common_buckets.clone(),
            )?,
            run_query: utils::register_histogram_metric(
                registry,
                RUN_QUERY_NAME,
                RUN_QUERY_HELP,
                common_buckets.clone(),
            )?,
            run_query_by_prefix: utils::register_histogram_metric(
                registry,
                RUN_QUERY_BY_PREFIX_NAME,
                RUN_QUERY_BY_PREFIX_HELP,
                common_buckets.clone(),
            )?,
            commit_step: utils::register_histogram_metric(
                registry,
                COMMIT_STEP_NAME,
                COMMIT_STEP_HELP,
                common_buckets.clone(),
            )?,
            commit_genesis: utils::register_histogram_metric(
                registry,
                COMMIT_GENESIS_NAME,
                COMMIT_GENESIS_HELP,
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
            get_total_supply: utils::register_histogram_metric(
                registry,
                GET_TOTAL_SUPPLY_NAME,
                GET_TOTAL_SUPPLY_HELP,
                common_buckets.clone(),
            )?,
            get_round_seigniorage_rate: utils::register_histogram_metric(
                registry,
                GET_ROUND_SEIGNIORAGE_RATE_NAME,
                GET_ROUND_SEIGNIORAGE_RATE_HELP,
                common_buckets.clone(),
            )?,
            get_era_validators: utils::register_histogram_metric(
                registry,
                GET_ERA_VALIDATORS_NAME,
                GET_ERA_VALIDATORS_HELP,
                common_buckets.clone(),
            )?,
            get_all_values: utils::register_histogram_metric(
                registry,
                GET_ALL_VALUES_NAME,
                GET_ALL_VALUES_NAME_HELP,
                common_buckets.clone(),
            )?,
            execution_results_checksum: utils::register_histogram_metric(
                registry,
                EXECUTION_RESULTS_CHECKSUM_NAME,
                EXECUTION_RESULTS_CHECKSUM_HELP,
                common_buckets.clone(),
            )?,
            addressable_entity: utils::register_histogram_metric(
                registry,
                ADDRESSABLE_ENTITY_NAME,
                ADDRESSABLE_ENTITY_HELP,
                common_buckets.clone(),
            )?,
            entry_points: utils::register_histogram_metric(
                registry,
                ENTRY_POINT_NAME,
                ENTRY_POINT_HELP,
                common_buckets,
            )?,
            get_trie: utils::register_histogram_metric(
                registry,
                GET_TRIE_NAME,
                GET_TRIE_HELP,
                tiny_buckets.clone(),
            )?,
            put_trie: utils::register_histogram_metric(
                registry,
                PUT_TRIE_NAME,
                PUT_TRIE_HELP,
                tiny_buckets,
            )?,
            latest_commit_step,
            exec_queue_size,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.exec_block_pre_processing);
        unregister_metric!(self.registry, self.exec_block_tnx_processing);
        unregister_metric!(self.registry, self.exec_wasm_v1);
        unregister_metric!(self.registry, self.exec_block_post_processing);
        unregister_metric!(self.registry, self.exec_block_step_processing);
        unregister_metric!(self.registry, self.exec_block_total);
        unregister_metric!(self.registry, self.commit_genesis);
        unregister_metric!(self.registry, self.commit_upgrade);
        unregister_metric!(self.registry, self.run_query);
        unregister_metric!(self.registry, self.run_query_by_prefix);
        unregister_metric!(self.registry, self.commit_step);
        unregister_metric!(self.registry, self.get_balance);
        unregister_metric!(self.registry, self.get_total_supply);
        unregister_metric!(self.registry, self.get_round_seigniorage_rate);
        unregister_metric!(self.registry, self.get_era_validators);
        unregister_metric!(self.registry, self.get_all_values);
        unregister_metric!(self.registry, self.execution_results_checksum);
        unregister_metric!(self.registry, self.put_trie);
        unregister_metric!(self.registry, self.get_trie);
        unregister_metric!(self.registry, self.latest_commit_step);
        unregister_metric!(self.registry, self.exec_queue_size);
        unregister_metric!(self.registry, self.entry_points);
    }
}
