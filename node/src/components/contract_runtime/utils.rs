use crate::{
    contract_runtime::{
        exec_queue::{ExecQueue, QueueItem},
        execute_finalized_block,
        metrics::Metrics,
        rewards, BlockAndExecutionResults, ExecutionPreState, StepEffectsAndUpcomingEraValidators,
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
    if executable_block.era_report.is_some() && executable_block.rewards.is_none() {
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
