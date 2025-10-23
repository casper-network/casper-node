use casper_executor_wasm::ExecutorV2;
use num_rational::Ratio;
use once_cell::sync::Lazy;
use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    ops::Range,
    sync::{Arc, Mutex},
    time::Instant,
};
use tracing::{debug, error, info};

use crate::{
    contract_runtime::{
        exec_queue::{ExecQueue, QueueItem},
        execute_finalized_block,
        metrics::Metrics,
        rewards,
        types::{BlockAndExecutionArtifacts, ExecutionPreState, StepOutcome},
        BlockExecutionError,
    },
    effect::{
        announcements::{
            ContractRuntimeAnnouncement, FatalAnnouncement, MetaBlockAnnouncement,
            NonExecutableBlockAnnouncement,
        },
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder,
    },
    fatal,
    types::{ExecutableBlock, MetaBlock, MetaBlockState},
};

use casper_binary_port::SpeculativeExecutionResult;
use casper_execution_engine::engine_state::{ExecutionEngineV1, WasmV1Result};
use casper_storage::{
    data_access_layer::{
        DataAccessLayer, FlushRequest, FlushResult, ProtocolUpgradeRequest, ProtocolUpgradeResult,
        TransferResult,
    },
    global_state::state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
};
use casper_types::{BlockHash, Chainspec, Digest, EraId, Gas, Key, ProtocolUpgradeConfig};

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

// Maybe era end processing instructions.
#[derive(Debug)]
enum EraEndInstruction {
    // Is not a switch block.
    ExecNonSwitch,
    // Is a switch block, and we can calc next era gas price, thus we can exec.
    ExecSwitch { next_gas_price: u8 },
    // Is a switch block, but we cannot execute.
    NoExec,
    // Fatal with error string.
    Fatal(String),
}

/// This currently handles reward and dynamic gas price calculation. If in future
/// similar end of era determinations need to be made, they should potentially
/// be added here.
async fn handle_era_end<REv>(
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
    chainspec: Arc<Chainspec>,
    metrics: Arc<Metrics>,
    effect_builder: EffectBuilder<REv>,
    executable_block: &mut ExecutableBlock,
) -> EraEndInstruction
where
    REv: From<ContractRuntimeRequest>
        + From<ContractRuntimeAnnouncement>
        + From<StorageRequest>
        + From<MetaBlockAnnouncement>
        + From<FatalAnnouncement>
        + Send,
{
    if executable_block.era_report.is_none() {
        return EraEndInstruction::ExecNonSwitch;
    }
    // this logic could be further broken down to each part if desired

    // reward stuff
    if executable_block.rewards.is_none() {
        executable_block.rewards = Some(if chainspec.core_config.compute_rewards {
            let rewards = match rewards::fetch_data_and_calculate_rewards_for_era(
                effect_builder,
                data_access_layer.clone(),
                chainspec.as_ref(),
                &metrics,
                executable_block.clone(),
            )
            .await
            {
                Ok(rewards) => rewards,
                Err(e) => {
                    return EraEndInstruction::Fatal(format!(
                        "Failed to compute the rewards: {e:?}"
                    ));
                }
            };

            debug!("rewards successfully computed");

            rewards
        } else {
            BTreeMap::new()
        });
    }

    // dynamic gas price stuff
    let era_id = executable_block.era_id;
    let block_height = executable_block.height;
    info!(%era_id, %block_height, "End of era calculating new gas price");

    if let Some(next_gas_price) = executable_block.next_era_gas_price {
        // keep up nodes are executing a block as determined by validators
        // and the next era gas price is already determined
        return EraEndInstruction::ExecSwitch { next_gas_price };
    }
    // we need to calculate the utilization of the block we are about to execute
    // and include it in the tally of the utilization for the entire era.
    let executable_block_utilization_score =
        match executable_block.calc_utilization_score(&chainspec) {
            Some(score) => score,
            None => {
                return EraEndInstruction::Fatal(format!(
                    "could not calc utilization of executable block {}",
                    block_height
                ));
            }
        };
    // BLOCKING CALL
    match effect_builder
        .get_era_utilization(era_id, block_height, executable_block_utilization_score)
        .await
    {
        Some((utilization, block_count, total_block_count)) => {
            if block_count != total_block_count {
                // The node needs awareness of all of the blocks for the era for which it tries to
                // produce the switch block.
                return EraEndInstruction::NoExec;
            }

            let current_gas_price = executable_block.current_gas_price;
            let era_score = { Ratio::new(utilization, block_count).to_integer() };

            let go_up = chainspec.vacancy_config.upper_threshold;
            let go_down = chainspec.vacancy_config.lower_threshold;
            let max = chainspec.vacancy_config.max_gas_price;
            let min = chainspec.vacancy_config.min_gas_price;
            let next_gas_price = if era_score >= go_up {
                current_gas_price.saturating_add(1).min(max)
            } else if era_score <= go_down {
                current_gas_price.saturating_sub(1).max(min)
            } else {
                current_gas_price
            };
            info!(%next_gas_price, "Calculated new gas price");
            EraEndInstruction::ExecSwitch { next_gas_price }
        }
        None => {
            let error = BlockExecutionError::FailedToGetNewEraGasPrice { era_id };
            EraEndInstruction::Fatal(format!("{}", error))
        }
    }
}

/// This function can fatal.
#[allow(clippy::too_many_arguments)]
pub(super) async fn exec_and_check_next<REv>(
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
    execution_engine_v1: Arc<ExecutionEngineV1>,
    execution_engine_v2: ExecutorV2,
    chainspec: Arc<Chainspec>,
    metrics: Arc<Metrics>,
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
        + From<NonExecutableBlockAnnouncement>
        + Send,
{
    debug!("ContractRuntime: execute_finalized_block_or_requeue");

    // FIRST determine if we are aware of the last switch block header
    let era_id = executable_block.era_id;
    let last_switch_block_hash = match era_id.predecessor() {
        Some(previous_era) => {
            let switch_block_header = effect_builder
                .get_switch_block_header_by_era_id_from_storage(previous_era)
                .await;
            if switch_block_header.is_none() {
                return fatal!(
                    effect_builder,
                    "switch block header can only be none for genesis era"
                )
                .await;
            }
            switch_block_header.map(|header| header.block_hash())
        }
        None => {
            // genesis era
            None
        }
    };

    let era_end_instruction = handle_era_end(
        data_access_layer.clone(),
        chainspec.clone(),
        metrics.clone(),
        effect_builder,
        &mut executable_block,
    )
    .await;
    debug!(?era_end_instruction, "era_end_instruction");
    let maybe_next_era_gas_price = match era_end_instruction {
        EraEndInstruction::ExecNonSwitch => None,
        EraEndInstruction::ExecSwitch { next_gas_price } => Some(next_gas_price),
        EraEndInstruction::NoExec => {
            // This means that we don't have enough data to calculate the era_end field
            // The best thing we can do here is force the node to CatchUp with the hope
            // that it will either acquire the missing state or the network will progress
            // and we will move past this point.
            info!(
                block_height = executable_block.height,
                "ContractRuntime: not enough data to execute switch block. Abandoning the execution."
            );
            effect_builder
                .announce_not_executing_block(executable_block.height)
                .await;
            return;
        }
        EraEndInstruction::Fatal(msg) => {
            return fatal!(effect_builder, "{}", msg).await;
        }
    };

    let current_gas_price = executable_block.current_gas_price;
    let contract_runtime_metrics = metrics.clone();
    let task = move || {
        debug!("ContractRuntime: execute_finalized_block");
        execute_finalized_block(
            data_access_layer.as_ref(),
            execution_engine_v1.as_ref(),
            execution_engine_v2,
            chainspec.as_ref(),
            Some(contract_runtime_metrics),
            current_pre_state,
            executable_block,
            key_block_height_for_activation_point,
            current_gas_price,
            maybe_next_era_gas_price,
            last_switch_block_hash,
        )
    };
    let BlockAndExecutionArtifacts {
        block,
        approvals_hashes,
        execution_artifacts,
        step_outcome: maybe_step_outcome,
    } = match run_intensive_task(task).await {
        Ok(ret) => ret,
        Err(error) => {
            error!(%error, "failed to execute block");
            return fatal!(effect_builder, "{}", error).await;
        }
    };

    // from this point onward we are dealing with the block we just created by executing
    let new_execution_pre_state = ExecutionPreState::from_block_header(block.header());
    {
        // The `shared_pre_state` could have been set to a block we just fully synced after
        // doing a sync leap (via a call to `set_execution_pre_state`).  We should not allow a block
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
    let block_height = block.height();

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
        debug!(
            %era_id,
            %block_height,
            "Storing block after execution"
        );
        effect_builder
            .put_executed_block_to_storage(Arc::clone(&block), approvals_hashes, artifacts_map)
            .await;
    } else {
        debug!(
            %era_id,
            %block_height,
            "Block was already stored before execution, storing approvals"
        );
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

    // TODO: if it is an error why allow it in the first place?
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

    if let Some(next_era_gas_price) = maybe_next_era_gas_price {
        effect_builder
            .announce_new_era_gas_price(current_era_id.successor(), next_era_gas_price)
            .await;
    }
    let meta_block = MetaBlock::new_forward(block, execution_artifacts, meta_block_state);
    effect_builder.announce_meta_block(meta_block).await;

    let next_block = exec_queue.remove(new_execution_pre_state.next_block_height());

    // We schedule the next block from the queue to be executed, if available.
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

pub(super) async fn handle_protocol_upgrade<REv>(
    effect_builder: EffectBuilder<REv>,
    data_access_layer: Arc<DataAccessLayer<LmdbGlobalState>>,
    metrics: Arc<Metrics>,
    upgrade_config: ProtocolUpgradeConfig,
    next_block_height: u64,
    parent_hash: BlockHash,
    parent_seed: Digest,
) where
    REv: From<ContractRuntimeRequest>
        + From<ContractRuntimeAnnouncement>
        + From<StorageRequest>
        + From<MetaBlockAnnouncement>
        + From<FatalAnnouncement>
        + Send,
{
    debug!(?upgrade_config, "upgrade");
    let start = Instant::now();
    let upgrade_request = ProtocolUpgradeRequest::new(upgrade_config);

    let result = run_intensive_task(move || {
        let result = data_access_layer.protocol_upgrade(upgrade_request);
        if result.is_success() {
            info!("committed upgrade");
            metrics
                .commit_upgrade
                .observe(start.elapsed().as_secs_f64());
            let flush_req = FlushRequest::new();
            if let FlushResult::Failure(err) = data_access_layer.flush(flush_req) {
                return Err(format!("{:?}", err));
            }
        }

        Ok(result)
    })
    .await;

    match result {
        Err(error_msg) => {
            // The only way this happens is if there is a problem in the flushing.
            error!(%error_msg, ":Error in post upgrade flush");
            fatal!(effect_builder, "{}", error_msg).await;
        }
        Ok(result) => match result {
            ProtocolUpgradeResult::RootNotFound => {
                let error_msg = "Root not found for protocol upgrade";
                fatal!(effect_builder, "{}", error_msg).await;
            }
            ProtocolUpgradeResult::Failure(err) => {
                fatal!(effect_builder, "{:?}", err).await;
            }
            ProtocolUpgradeResult::Success {
                post_state_hash, ..
            } => {
                let post_upgrade_state = ExecutionPreState::new(
                    next_block_height,
                    post_state_hash,
                    parent_hash,
                    parent_seed,
                );

                effect_builder
                    .update_contract_runtime_state(post_upgrade_state)
                    .await
            }
        },
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

pub(crate) fn spec_exec_from_transfer_result(
    limit: Gas,
    transfer_result: TransferResult,
    block_hash: BlockHash,
) -> SpeculativeExecutionResult {
    let transfers = transfer_result.transfers().to_owned();
    let consumed = limit;
    let effects = transfer_result.effects().to_owned();
    let messages = vec![];
    let error_msg = transfer_result
        .error()
        .to_owned()
        .map(|err| format!("{:?}", err));

    SpeculativeExecutionResult::new(
        block_hash, transfers, limit, consumed, effects, messages, error_msg,
    )
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
        // batch out of u64::MAX of era info needs to iterate over all chunks.
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
