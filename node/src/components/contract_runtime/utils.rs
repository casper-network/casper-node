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
use casper_types::{Chainspec, EraId, Key};
use once_cell::sync::Lazy;
use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    ops::Range,
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
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
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
            data_access_layer.as_ref(),
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

fn generate_range_by_index(
    highest_era: u64,
    batch_size: u64,
    batch_index: u64,
) -> Option<Range<u64>> {
    let start = batch_index.checked_mul(batch_size)?;
    let end = cmp::min(start.checked_add(batch_size)?, highest_era);
    Some(start..end)
}

/// Calculates era keys to be pruned.
///
/// Outcomes:
/// * Ok(Some(range)) -- these keys should be pruned
/// * Ok(None) -- nothing to do, either done, or there is not enough eras to prune
pub(super) fn calculate_prune_eras(
    activation_era_id: EraId,
    activation_height: u64,
    current_height: u64,
    batch_size: u64,
) -> Option<Vec<Key>> {
    if batch_size == 0 {
        // Nothing to do, the batch size is 0.
        return None;
    }

    let nth_chunk: u64 = match current_height.checked_sub(activation_height) {
        Some(nth_chunk) => nth_chunk,
        None => {
            // Time went backwards, programmer error, etc
            error!(
                %activation_era_id,
                activation_height,
                current_height,
                batch_size,
                "unable to calculate eras to prune (activation height higher than the block height)"
            );
            panic!("activation height higher than the block height");
        }
    };

    let range = generate_range_by_index(activation_era_id.value(), batch_size, nth_chunk)?;

    if range.is_empty() {
        return None;
    }

    Some(range.map(EraId::new).map(Key::EraInfo).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculation_is_safe_with_invalid_input() {
        assert_eq!(calculate_prune_eras(EraId::new(0), 0, 0, 0,), None);
        assert_eq!(calculate_prune_eras(EraId::new(0), 0, 0, 5,), None);
        assert_eq!(calculate_prune_eras(EraId::new(u64::MAX), 0, 0, 0,), None);
        assert_eq!(
            calculate_prune_eras(EraId::new(u64::MAX), 1, u64::MAX, u64::MAX),
            None
        );
    }

    #[test]
    fn calculation_is_lazy() {
        // NOTE: Range of EraInfos is lazy, so it does not consume memory, but getting the last
        // batch out of u64::MAX of erainfos needs to iterate over all chunks.
        assert!(calculate_prune_eras(EraId::new(u64::MAX), 0, u64::MAX, 100,).is_none(),);
        assert_eq!(
            calculate_prune_eras(EraId::new(u64::MAX), 1, 100, 100,)
                .unwrap()
                .len(),
            100
        );
    }

    #[test]
    fn should_calculate_prune_eras() {
        let activation_height = 50;
        let current_height = 50;
        const ACTIVATION_POINT_ERA_ID: EraId = EraId::new(5);

        // batch size 1

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                1
            ),
            Some(vec![Key::EraInfo(EraId::new(0))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                1
            ),
            Some(vec![Key::EraInfo(EraId::new(1))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                1
            ),
            Some(vec![Key::EraInfo(EraId::new(2))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 3,
                1
            ),
            Some(vec![Key::EraInfo(EraId::new(3))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 4,
                1
            ),
            Some(vec![Key::EraInfo(EraId::new(4))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 5,
                1
            ),
            None,
        );
        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT_ERA_ID, activation_height, u64::MAX, 1),
            None,
        );

        // batch size 2

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                2
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1))
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                2
            ),
            Some(vec![
                Key::EraInfo(EraId::new(2)),
                Key::EraInfo(EraId::new(3))
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                2
            ),
            Some(vec![Key::EraInfo(EraId::new(4))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 3,
                2
            ),
            None
        );
        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT_ERA_ID, activation_height, u64::MAX, 2),
            None,
        );

        // batch size 3

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                3
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1)),
                Key::EraInfo(EraId::new(2)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                3
            ),
            Some(vec![
                Key::EraInfo(EraId::new(3)),
                Key::EraInfo(EraId::new(4)),
            ])
        );

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                3
            ),
            None
        );
        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT_ERA_ID, activation_height, u64::MAX, 3),
            None,
        );

        // batch size 4

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                4
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1)),
                Key::EraInfo(EraId::new(2)),
                Key::EraInfo(EraId::new(3)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                4
            ),
            Some(vec![Key::EraInfo(EraId::new(4)),])
        );

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                4
            ),
            None
        );
        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT_ERA_ID, activation_height, u64::MAX, 4),
            None,
        );

        // batch size 5

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                5
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1)),
                Key::EraInfo(EraId::new(2)),
                Key::EraInfo(EraId::new(3)),
                Key::EraInfo(EraId::new(4)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                5
            ),
            None,
        );

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                5
            ),
            None
        );
        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT_ERA_ID, activation_height, u64::MAX, 5),
            None,
        );

        // batch size 6

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                6
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1)),
                Key::EraInfo(EraId::new(2)),
                Key::EraInfo(EraId::new(3)),
                Key::EraInfo(EraId::new(4)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                6
            ),
            None,
        );

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                6
            ),
            None
        );
        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT_ERA_ID, activation_height, u64::MAX, 6),
            None,
        );

        // batch size max

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height,
                u64::MAX,
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1)),
                Key::EraInfo(EraId::new(2)),
                Key::EraInfo(EraId::new(3)),
                Key::EraInfo(EraId::new(4)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                u64::MAX,
            ),
            None,
        );

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                u64::MAX,
            ),
            None
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                u64::MAX,
                u64::MAX,
            ),
            None,
        );
    }
}
