use num_rational::Ratio;
use once_cell::sync::Lazy;
use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    ops::Range,
    sync::{Arc, Mutex},
};
use tracing::{debug, error, info, warn};

use crate::{
    contract_runtime::{
        exec_queue::{ExecQueue, QueueItem},
        execute_finalized_block,
        metrics::Metrics,
        rewards, BlockAndExecutionArtifacts, BlockExecutionError, ExecutionPreState, StepOutcome,
    },
    effect::{
        announcements::{ContractRuntimeAnnouncement, FatalAnnouncement, MetaBlockAnnouncement},
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder,
    },
    fatal,
    types::{ExecutableBlock, MetaBlock, MetaBlockState},
};

use casper_binary_port::SpeculativeExecutionResult;
use casper_execution_engine::engine_state::{ExecutionEngineV1, WasmV1Result};
use casper_storage::{
    data_access_layer::DataAccessLayer, global_state::state::lmdb::LmdbGlobalState,
};
use casper_types::{BlockHash, Chainspec, EraId, GasLimited, Key};

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
    V: 'static + Send + Debug,
{
    // This will never panic since the semaphore is never closed.
    let _permit = INTENSIVE_TASKS_SEMAPHORE.acquire().await.unwrap();
    let result = tokio::task::spawn_blocking(task).await;
    match result {
        Ok(ret) => ret,
        Err(err) => {
            error!("{:?}", err);
            panic!("intensive contract runtime task errored: {:?}", err);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn exec_or_requeue<REv>(
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
    execution_engine_v1: Arc<ExecutionEngineV1>,
    chainspec: Arc<Chainspec>,
    metrics: Arc<Metrics>,
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
    let is_era_end = executable_block.era_report.is_some();
    if is_era_end && executable_block.rewards.is_none() {
        executable_block.rewards = Some(if chainspec.core_config.compute_rewards {
            let rewards = match rewards::fetch_data_and_calculate_rewards_for_era(
                effect_builder,
                data_access_layer.clone(),
                chainspec.as_ref(),
                executable_block.clone(),
            )
            .await
            {
                Ok(rewards) => rewards,
                Err(e) => {
                    return fatal!(effect_builder, "Failed to compute the rewards: {e:?}").await;
                }
            };

            debug!("rewards successfully computed");

            rewards
        } else {
            BTreeMap::new()
        });
    }

    let maybe_next_era_gas_price = if is_era_end && executable_block.next_era_gas_price.is_none() {
        let max_block_size = chainspec.transaction_config.max_block_size as u64;
        let block_gas_limit = chainspec.transaction_config.block_gas_limit;
        let go_up = chainspec.vacancy_config.upper_threshold;
        let go_down = chainspec.vacancy_config.lower_threshold;
        let max = chainspec.vacancy_config.max_gas_price;
        let min = chainspec.vacancy_config.min_gas_price;
        info!("End of era calculating new gas price");
        let era_id = executable_block.era_id;
        let block_height = executable_block.height;

        let per_block_capacity = chainspec
            .transaction_config
            .transaction_v1_config
            .get_max_block_count();

        let switch_block_utilization_score = {
            let mut has_hit_slot_limt = false;

            for (category, transactions) in executable_block.transaction_map.iter() {
                let max_count = chainspec
                    .transaction_config
                    .transaction_v1_config
                    .get_max_transaction_count(*category);
                if max_count == transactions.len() as u64 {
                    has_hit_slot_limt = true;
                }
            }

            if has_hit_slot_limt {
                100u64
            } else if executable_block.transactions.is_empty() {
                0u64
            } else {
                let size_utilization: u64 = {
                    let total_size_of_transactions: u64 = executable_block
                        .transactions
                        .iter()
                        .map(|transaction| transaction.size_estimate() as u64)
                        .sum();

                    Ratio::new(total_size_of_transactions * 100, max_block_size).to_integer()
                };

                let gas_utilization: u64 = {
                    let total_gas_limit: u64 = executable_block
                        .transactions
                        .iter()
                        .map(|transaction| match transaction.gas_limit(&chainspec) {
                            Ok(gas_limit) => gas_limit.value().as_u64(),
                            Err(_) => {
                                warn!("Unable to determine gas limit");
                                0u64
                            }
                        })
                        .sum();

                    Ratio::new(total_gas_limit * 100, block_gas_limit).to_integer()
                };

                let slot_utilization = Ratio::new(
                    executable_block.transactions.len() as u64 * 100,
                    per_block_capacity,
                )
                .to_integer();

                let uitilization_scores = vec![slot_utilization, gas_utilization, size_utilization];

                match uitilization_scores.iter().max() {
                    Some(max_score) => *max_score,
                    None => {
                        let error = BlockExecutionError::FailedToGetNewEraGasPrice { era_id };
                        return fatal!(effect_builder, "{}", error).await;
                    }
                }
            }
        };

        let maybe_utilization = effect_builder
            .get_block_utilization(era_id, block_height, switch_block_utilization_score)
            .await;

        match maybe_utilization {
            None => {
                let error = BlockExecutionError::FailedToGetNewEraGasPrice { era_id };
                return fatal!(effect_builder, "{}", error).await;
            }
            Some((utilization, block_count)) => {
                let era_score = { Ratio::new(utilization, block_count).to_integer() };

                let new_gas_price = if era_score >= go_up {
                    let new_gas_price = current_gas_price + 1;
                    if new_gas_price > max {
                        max
                    } else {
                        new_gas_price
                    }
                } else if era_score <= go_down {
                    let new_gas_price = current_gas_price - 1;
                    if new_gas_price <= min {
                        min
                    } else {
                        new_gas_price
                    }
                } else {
                    current_gas_price
                };
                info!(%new_gas_price, "Calculated new gas price");
                Some(new_gas_price)
            }
        }
    } else if executable_block.next_era_gas_price.is_some() {
        executable_block.next_era_gas_price
    } else {
        None
    };

    let era_id = executable_block.era_id;

    let last_switch_block_hash = if let Some(previous_era) = era_id.predecessor() {
        let switch_block_header = effect_builder
            .get_switch_block_header_by_era_id_from_storage(previous_era)
            .await;
        switch_block_header.map(|header| header.block_hash())
    } else {
        None
    };

    let BlockAndExecutionArtifacts {
        block,
        approvals_hashes,
        execution_artifacts,
        step_outcome: maybe_step_outcome,
    } = match run_intensive_task(move || {
        debug!("ContractRuntime: execute_finalized_block");
        execute_finalized_block(
            data_access_layer.as_ref(),
            execution_engine_v1.as_ref(),
            chainspec.as_ref(),
            Some(contract_runtime_metrics),
            current_pre_state,
            executable_block,
            key_block_height_for_activation_point,
            current_gas_price,
            maybe_next_era_gas_price,
            last_switch_block_hash,
        )
    })
    .await
    {
        Ok(ret) => ret,
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

    if let Some(StepOutcome {
        step_effects,
        mut upcoming_era_validators,
    }) = maybe_step_outcome
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

    debug!(
        block_hash = %block.hash(),
        height = block.height(),
        era = block.era_id().value(),
        is_switch_block = block.is_switch_block(),
        "executed block"
    );

    let artifacts_map: HashMap<_, _> = execution_artifacts
        .iter()
        .cloned()
        .map(|artifact| (artifact.transaction_hash, artifact.execution_result))
        .collect();

    if meta_block_state.register_as_stored().was_updated() {
        effect_builder
            .put_executed_block_to_storage(Arc::clone(&block), approvals_hashes, artifacts_map)
            .await;
    } else {
        effect_builder
            .put_approvals_hashes_to_storage(approvals_hashes)
            .await;
        effect_builder
            .put_execution_artifacts_to_storage(
                *block.hash(),
                block.height(),
                block.era_id(),
                artifacts_map,
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

    let meta_block = MetaBlock::new_forward(block, execution_artifacts, meta_block_state);
    effect_builder.announce_meta_block(meta_block).await;

    // If the child is already finalized, start execution.
    let next_block = exec_queue.remove(new_execution_pre_state.next_block_height());

    if let Some(next_era_gas_price) = maybe_next_era_gas_price {
        effect_builder
            .announce_new_era_gas_price(current_era_id.successor(), next_era_gas_price)
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

pub(crate) fn spec_exec_from_wasm_v1_result(
    wasm_v1_result: WasmV1Result,
    block_hash: BlockHash,
) -> SpeculativeExecutionResult {
    let transfers = wasm_v1_result.transfers().to_owned();
    let limit = wasm_v1_result.limit().to_owned();
    let consumed = wasm_v1_result.consumed().to_owned();
    let effects = wasm_v1_result.effects().to_owned();
    let messages = wasm_v1_result.messages().to_owned();
    let error_msg = wasm_v1_result
        .error()
        .to_owned()
        .map(|err| format!("{:?}", err));

    SpeculativeExecutionResult::new(
        block_hash, transfers, limit, consumed, effects, messages, error_msg,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculation_is_safe_with_invalid_input() {
        assert_eq!(calculate_prune_eras(EraId::new(0), 0, 0, 0), None);
        assert_eq!(calculate_prune_eras(EraId::new(0), 0, 0, 5), None);
        assert_eq!(calculate_prune_eras(EraId::new(u64::MAX), 0, 0, 0), None);
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
            calculate_prune_eras(EraId::new(u64::MAX), 1, 100, 100)
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
                1,
            ),
            Some(vec![Key::EraInfo(EraId::new(0))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                1,
            ),
            Some(vec![Key::EraInfo(EraId::new(1))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                1,
            ),
            Some(vec![Key::EraInfo(EraId::new(2))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 3,
                1,
            ),
            Some(vec![Key::EraInfo(EraId::new(3))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 4,
                1,
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
                2,
            ),
            Some(vec![
                Key::EraInfo(EraId::new(0)),
                Key::EraInfo(EraId::new(1)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 1,
                2,
            ),
            Some(vec![
                Key::EraInfo(EraId::new(2)),
                Key::EraInfo(EraId::new(3)),
            ])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                2,
            ),
            Some(vec![Key::EraInfo(EraId::new(4))])
        );
        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 3,
                2,
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
                3,
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
                3,
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
                3,
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
                4,
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
                4,
            ),
            Some(vec![Key::EraInfo(EraId::new(4))])
        );

        assert_eq!(
            calculate_prune_eras(
                ACTIVATION_POINT_ERA_ID,
                activation_height,
                current_height + 2,
                4,
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
                5,
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
                5,
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
                6,
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
                6,
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
