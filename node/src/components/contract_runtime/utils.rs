use crate::{
    contract_runtime::{
        exec_queue::{ExecQueue, QueueItem},
        execute_finalized_block,
        metrics::Metrics,
        rewards, BlockAndExecutionResults, BlockExecutionError, ExecutionPreState,
        StepEffectsAndUpcomingEraValidators,
    },
    effect::{
        announcements::{ContractRuntimeAnnouncement, FatalAnnouncement, MetaBlockAnnouncement},
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder,
    },
    fatal,
    types::{ExecutableBlock, MetaBlock, MetaBlockState},
};
use casper_execution_engine::engine_state::EngineState;
use casper_storage::{
    data_access_layer::DataAccessLayer, global_state::state::lmdb::LmdbGlobalState,
};
use casper_types::{Chainspec, EraId};
use datasize::DataSize;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};
use tracing::{debug, error, info};

/// Maximum number of resource intensive tasks that can be run in parallel.
///
/// TODO: Fine tune this constant to the machine executing the node.
const MAX_PARALLEL_INTENSIVE_TASKS: usize = 4;
/// Semaphore enforcing maximum number of parallel resource intensive tasks.
static INTENSIVE_TASKS_SEMAPHORE: Lazy<tokio::sync::Semaphore> =
    Lazy::new(|| tokio::sync::Semaphore::new(MAX_PARALLEL_INTENSIVE_TASKS));

/// Asynchronously runs a resource intensive task.
/// At most `MAX_PARALLEL_INTENSIVE_TASKS` are being run in parallel at any time.
///
/// The task is a closure that takes no arguments and returns a value.
/// This function returns a future for that value.
pub(super) async fn run_intensive_task<T, V>(task: T) -> V
where
    T: 'static + Send + FnOnce() -> V,
    V: 'static + Send,
{
    // This will never panic since the semaphore is never closed.
    let _permit = INTENSIVE_TASKS_SEMAPHORE.acquire().await.unwrap();
    tokio::task::spawn_blocking(task)
        .await
        .expect("task panicked")
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn exec_or_requeue<REv>(
    engine_state: Arc<EngineState<DataAccessLayer<LmdbGlobalState>>>,
    metrics: Arc<Metrics>,
    chainspec: Arc<Chainspec>,
    mut exec_queue: ExecQueue,
    shared_pre_state: Arc<Mutex<ExecutionPreState>>,
    current_pre_state: ExecutionPreState,
    effect_builder: EffectBuilder<REv>,
    mut executable_block: ExecutableBlock,
    key_block_height_for_activation_point: u64,
    mut meta_block_state: MetaBlockState,
    current_gas_price: u8,
) where
    REv: From<ContractRuntimeRequest>
        + From<ContractRuntimeAnnouncement>
        + From<StorageRequest>
        + From<MetaBlockAnnouncement>
        + From<FatalAnnouncement>
        + Send,
{
    debug!("ContractRuntime: execute_finalized_block_or_requeue");
    let contract_runtime_metrics = metrics.clone();

    let protocol_version = chainspec.protocol_version();
    let activation_point = chainspec.protocol_config.activation_point;
    let prune_batch_size = chainspec.core_config.prune_batch_size;
    let block_max_install_upgrade_count =
        chainspec.transaction_config.block_max_install_upgrade_count;
    let block_max_standard_count = chainspec.transaction_config.block_max_standard_count;
    let block_max_transfer_count = chainspec.transaction_config.block_max_transfer_count;
    let block_max_staking_count = chainspec.transaction_config.block_max_staking_count;
    let go_up = chainspec.transaction_config.go_up;
    let go_down = chainspec.transaction_config.go_down;
    let max = chainspec.transaction_config.max;
    let min = chainspec.transaction_config.min;
    let is_era_end = executable_block.era_report.is_some();
    if is_era_end && executable_block.rewards.is_none() {
        executable_block.rewards = Some(if chainspec.core_config.compute_rewards {
            let rewards = match rewards::fetch_data_and_calculate_rewards_for_era(
                effect_builder,
                chainspec,
                executable_block.clone(),
            )
            .await
            {
                Ok(rewards) => rewards,
                Err(e) => {
                    return fatal!(effect_builder, "Failed to compute the rewards: {e:?}").await
                }
            };

            info!("rewards successfully computed");

            rewards
        } else {
            //TODO instead, use a list of all the validators with 0
            BTreeMap::new()
        });
    }

    // TODO: Make the call to determine gas price calc based on the tracking in storage here!
    let maybe_next_era_gas_price = if is_era_end {
        info!("End of era calculating new gas price");
        let era_id = executable_block.era_id;
        let block_height = executable_block.height;

        let switch_block_transaction_hashes = executable_block.transactions.len() as u64;

        let maybe_utilization = effect_builder
            .get_block_utilization(era_id, block_height, switch_block_transaction_hashes)
            .await;

        match maybe_utilization {
            None => {
                let error = BlockExecutionError::FailedToGetNewEraGasPrice { era_id };
                return fatal!(effect_builder, "{}", error).await;
            }
            Some((utilization, block_count)) => {
                let per_block_capacity = {
                    block_max_install_upgrade_count
                        + block_max_standard_count
                        + block_max_transfer_count
                        + block_max_staking_count
                } as u64;

                let era_score = {
                    let numerator = utilization * 100;
                    let denominator = per_block_capacity * block_count;
                    Ratio::new(numerator, denominator).to_integer()
                };

                println!("Fooo {era_score}");

                let new_gas_price = if era_score >= go_up {
                    let new_gas_price = current_gas_price + 1;
                    if current_gas_price > max {
                        max
                    } else {
                        new_gas_price
                    }
                } else if era_score < go_down {
                    let new_gas_price = current_gas_price - 1;
                    if current_gas_price <= min {
                        min
                    } else {
                        new_gas_price
                    }
                } else {
                    current_gas_price
                };
                info!("Calculated new gas price");
                new_gas_price
            }
        }
    } else {
        current_gas_price
    };

    let BlockAndExecutionResults {
        block,
        approvals_hashes,
        execution_results,
        maybe_step_effects_and_upcoming_era_validators,
    } = match run_intensive_task(move || {
        debug!("ContractRuntime: execute_finalized_block");
        execute_finalized_block(
            engine_state.as_ref(),
            Some(contract_runtime_metrics),
            protocol_version,
            current_pre_state,
            executable_block,
            activation_point.era_id(),
            key_block_height_for_activation_point,
            prune_batch_size,
            maybe_next_era_gas_price,
        )
    })
    .await
    {
        Ok(block_and_execution_results) => block_and_execution_results,
        Err(error) => {
            error!(%error, "failed to execute block");
            return fatal!(effect_builder, "{}", error).await;
        }
    };

    let new_execution_pre_state = ExecutionPreState::from_block_header(block.header());
    {
        // The `shared_pre_state` could have been set to a block we just fully synced after
        // doing a sync leap (via a call to `set_initial_state`).  We should not allow a block
        // which completed execution just after this to set the `shared_pre_state` back to an
        // earlier block height.
        let mut shared_pre_state = shared_pre_state.lock().unwrap();
        if shared_pre_state.next_block_height() < new_execution_pre_state.next_block_height() {
            debug!(
                next_block_height = new_execution_pre_state.next_block_height(),
                "ContractRuntime: updating shared pre-state",
            );
            *shared_pre_state = new_execution_pre_state.clone();
        } else {
            debug!(
                current_next_block_height = shared_pre_state.next_block_height(),
                attempted_next_block_height = new_execution_pre_state.next_block_height(),
                "ContractRuntime: not updating shared pre-state to older state"
            );
        }
    }

    let current_era_id = block.era_id();

    if let Some(StepEffectsAndUpcomingEraValidators {
        step_effects,
        mut upcoming_era_validators,
    }) = maybe_step_effects_and_upcoming_era_validators
    {
        effect_builder
            .announce_commit_step_success(current_era_id, step_effects)
            .await;

        if current_era_id.is_genesis() {
            match upcoming_era_validators
                .get(&current_era_id.successor())
                .cloned()
            {
                Some(era_validators) => {
                    upcoming_era_validators.insert(EraId::default(), era_validators);
                }
                None => {
                    fatal!(effect_builder, "Missing era 1 validators").await;
                }
            }
        }

        effect_builder
            .announce_upcoming_era_validators(current_era_id, upcoming_era_validators)
            .await;
    }

    info!(
        block_hash = %block.hash(),
        height = block.height(),
        era = block.era_id().value(),
        is_switch_block = block.is_switch_block(),
        "executed block"
    );

    let execution_results_map: HashMap<_, _> = execution_results
        .iter()
        .cloned()
        .map(|artifact| (artifact.deploy_hash.into(), artifact.execution_result))
        .collect();
    if meta_block_state.register_as_stored().was_updated() {
        effect_builder
            .put_executed_block_to_storage(
                Arc::clone(&block),
                approvals_hashes,
                execution_results_map,
            )
            .await;
    } else {
        effect_builder
            .put_approvals_hashes_to_storage(approvals_hashes)
            .await;
        effect_builder
            .put_execution_results_to_storage(
                *block.hash(),
                block.height(),
                block.era_id(),
                execution_results_map,
            )
            .await;
    }
    if meta_block_state
        .register_as_executed()
        .was_already_registered()
    {
        error!(
            block_hash = %block.hash(),
            block_height = block.height(),
            ?meta_block_state,
            "should not execute the same block more than once"
        );
    }

    let meta_block = MetaBlock::new_forward(block, execution_results, meta_block_state);
    effect_builder.announce_meta_block(meta_block).await;

    // If the child is already finalized, start execution.
    let next_block = exec_queue.remove(new_execution_pre_state.next_block_height());

    if is_era_end {
        effect_builder
            .announce_new_era_gas_price(current_era_id.successor(), maybe_next_era_gas_price)
            .await;
    }

    // We schedule the next block from the queue to be executed:
    if let Some(QueueItem {
        executable_block,
        meta_block_state,
    }) = next_block
    {
        metrics.exec_queue_size.dec();
        debug!("ContractRuntime: next block enqueue_block_for_execution");
        effect_builder
            .enqueue_block_for_execution(executable_block, meta_block_state)
            .await;
    }
}

#[derive(Clone, Copy, Ord, Eq, PartialOrd, PartialEq, DataSize)]
pub(super) struct EraPrice {
    era_id: EraId,
    gas_price: u8,
}

impl EraPrice {
    pub(super) fn new(era_id: EraId, gas_price: u8) -> Self {
        Self { era_id, gas_price }
    }

    pub(super) fn gas_price(&self) -> u8 {
        self.gas_price
    }

    pub(super) fn maybe_gas_price_for_era_id(&self, era_id: EraId) -> Option<u8> {
        if self.era_id == era_id {
            return Some(self.gas_price);
        }

        None
    }
}
