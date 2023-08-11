use std::{cmp, collections::BTreeMap, ops::Range, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::{
    engine_state::{
        self, execution_result::ExecutionResults, step::EvictItem, ChecksumRegistry, DeployItem,
        EngineState, ExecuteRequest, ExecutionResult as EngineExecutionResult,
        GetEraValidatorsRequest, PruneConfig, PruneResult, StepError, StepRequest, StepSuccess,
    },
    execution,
};
use casper_storage::{
    data_access_layer::DataAccessLayer,
    global_state::{
        shared::{transform::Transform, AdditiveMap},
        state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
    },
};
use casper_types::{
    Block, CLValue, Deploy, DeployHash, DeployHeader, Digest, EraEnd, EraId, EraReport,
    ExecutionResult, Key, ProtocolVersion, PublicKey, U512,
};

use crate::{
    components::{
        contract_runtime::{
            error::BlockExecutionError, types::StepEffectAndUpcomingEraValidators,
            BlockAndExecutionResults, ExecutionPreState, Metrics, SpeculativeExecutionState,
            APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME,
        },
        fetcher::FetchItem,
    },
    types::{self, ApprovalsHashes, Chunkable, FinalizedBlock},
};

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
fn calculate_prune_eras(
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

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    engine_state: &EngineState<DataAccessLayer<LmdbGlobalState>>,
    metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    execution_pre_state: ExecutionPreState,
    finalized_block: FinalizedBlock,
    deploys: Vec<Deploy>,
    activation_point_era_id: EraId,
    key_block_height_for_activation_point: u64,
    prune_batch_size: u64,
) -> Result<BlockAndExecutionResults, BlockExecutionError> {
    if finalized_block.height != execution_pre_state.next_block_height {
        return Err(BlockExecutionError::WrongBlockHeight {
            finalized_block: Box::new(finalized_block),
            execution_pre_state: Box::new(execution_pre_state),
        });
    }
    let ExecutionPreState {
        pre_state_root_hash,
        parent_hash,
        parent_seed,
        next_block_height,
    } = execution_pre_state;
    let mut state_root_hash = pre_state_root_hash;
    let mut execution_results: Vec<(_, DeployHeader, ExecutionResult)> =
        Vec::with_capacity(deploys.len());
    // Run any deploys that must be executed
    let block_time = finalized_block.timestamp.millis();
    let start = Instant::now();
    let deploy_ids = deploys.iter().map(|deploy| deploy.fetch_id()).collect_vec();
    let approvals_checksum = types::compute_approvals_checksum(deploy_ids.clone())
        .map_err(BlockExecutionError::FailedToComputeApprovalsChecksum)?;

    // Create a new EngineState that reads from LMDB but only caches changes in memory.
    let scratch_state = engine_state.get_scratch_engine_state();

    // Pay out block rewards
    let proposer = *finalized_block.proposer.clone();
    state_root_hash = scratch_state.distribute_block_rewards(
        state_root_hash,
        protocol_version,
        proposer.clone(),
        next_block_height,
        block_time,
    )?;

    // WARNING: Do not change the order of `deploys` as it will result in a different root hash.
    for deploy in deploys {
        let deploy_hash = *deploy.hash();
        let deploy_header = deploy.header().clone();
        let execute_request = ExecuteRequest::new(
            state_root_hash,
            block_time,
            vec![DeployItem::from(deploy)],
            protocol_version,
            proposer.clone(),
        );

        // TODO: this is currently working coincidentally because we are passing only one
        // deploy_item per exec. The execution results coming back from the EE lack the
        // mapping between deploy_hash and execution result, and this outer logic is
        // enriching it with the deploy hash. If we were passing multiple deploys per exec
        // the relation between the deploy and the execution results would be lost.
        let result = execute(&scratch_state, metrics.clone(), execute_request)?;

        trace!(?deploy_hash, ?result, "deploy execution result");
        // As for now a given state is expected to exist.
        let (state_hash, execution_result) = commit_execution_results(
            &scratch_state,
            metrics.clone(),
            state_root_hash,
            deploy_hash,
            result,
        )?;
        execution_results.push((deploy_hash, deploy_header, execution_result));
        state_root_hash = state_hash;
    }

    // Write the deploy approvals and execution results Merkle root hashes to global state if there
    // were any deploys.
    let execution_results_checksum = compute_execution_results_checksum(
        &execution_results
            .iter()
            .map(|(_, _, result)| result)
            .cloned()
            .collect(),
    )?;

    let mut effects = AdditiveMap::new();
    let mut checksum_registry = ChecksumRegistry::new();
    checksum_registry.insert(APPROVALS_CHECKSUM_NAME, approvals_checksum);
    checksum_registry.insert(EXECUTION_RESULTS_CHECKSUM_NAME, execution_results_checksum);
    let _ = effects.insert(
        Key::ChecksumRegistry,
        Transform::Write(
            CLValue::from_t(checksum_registry)
                .map_err(BlockExecutionError::ChecksumRegistryToCLValue)?
                .into(),
        ),
    );
    scratch_state.commit_effects(state_root_hash, effects)?;

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // If the finalized block has an era report, run the auction contract and get the upcoming era
    // validators.
    let maybe_step_effect_and_upcoming_era_validators =
        if let Some(era_report) = finalized_block.era_report.as_deref() {
            let StepSuccess {
                post_state_hash: _, // ignore the post-state-hash returned from scratch
                execution_journal: step_execution_journal,
            } = commit_step(
                &scratch_state, // engine_state
                metrics,
                protocol_version,
                state_root_hash,
                era_report,
                finalized_block.timestamp.millis(),
                finalized_block.era_id.successor(),
            )?;

            state_root_hash =
                engine_state.write_scratch_to_db(state_root_hash, scratch_state.into_inner())?;

            // In this flow we execute using a recent state root hash where the system contract
            // registry is guaranteed to exist.
            let system_contract_registry = None;

            let upcoming_era_validators = engine_state.get_era_validators(
                system_contract_registry,
                GetEraValidatorsRequest::new(state_root_hash, protocol_version),
            )?;
            Some(StepEffectAndUpcomingEraValidators {
                step_execution_journal,
                upcoming_era_validators,
            })
        } else {
            // Finally, the new state-root-hash from the cumulative changes to global state is
            // returned when they are written to LMDB.
            state_root_hash =
                engine_state.write_scratch_to_db(state_root_hash, scratch_state.into_inner())?;
            None
        };

    // Flush once, after all deploys have been executed.
    engine_state.flush_environment()?;

    // Pruning
    if let Some(previous_block_height) = finalized_block.height.checked_sub(1) {
        if let Some(keys_to_prune) = calculate_prune_eras(
            activation_point_era_id,
            key_block_height_for_activation_point,
            previous_block_height,
            prune_batch_size,
        ) {
            let first_key = keys_to_prune.first().copied();
            let last_key = keys_to_prune.last().copied();
            info!(
                previous_block_height,
                %key_block_height_for_activation_point,
                %state_root_hash,
                first_key=?first_key,
                last_key=?last_key,
                "commit prune: preparing prune config"
            );
            let prune_config = PruneConfig::new(state_root_hash, keys_to_prune);
            match engine_state.commit_prune(prune_config) {
                Ok(PruneResult::RootNotFound) => {
                    error!(
                        previous_block_height,
                        %state_root_hash,
                        "commit prune: root not found"
                    );
                    panic!(
                        "Root {} not found while performing a prune.",
                        state_root_hash
                    );
                }
                Ok(PruneResult::DoesNotExist) => {
                    warn!(
                        previous_block_height,
                        %state_root_hash,
                        "commit prune: key does not exist"
                    );
                }
                Ok(PruneResult::Success {
                    post_state_hash, ..
                }) => {
                    info!(
                        previous_block_height,
                        %key_block_height_for_activation_point,
                        %state_root_hash,
                        %post_state_hash,
                        first_key=?first_key,
                        last_key=?last_key,
                        "commit prune: success"
                    );
                    state_root_hash = post_state_hash;
                }
                Err(error) => {
                    error!(
                        previous_block_height,
                        %key_block_height_for_activation_point,
                        %error,
                        "commit prune: commit prune error"
                    );
                    return Err(error.into());
                }
            }
        }
    }

    let maybe_next_era_validator_weights: Option<BTreeMap<PublicKey, U512>> =
        maybe_step_effect_and_upcoming_era_validators
            .as_ref()
            .and_then(
                |StepEffectAndUpcomingEraValidators {
                     upcoming_era_validators,
                     ..
                 }| {
                    upcoming_era_validators
                        .get(&finalized_block.era_id.successor())
                        .cloned()
                },
            );

    let era_end = match (finalized_block.era_report, maybe_next_era_validator_weights) {
        (None, None) => None,
        (Some(era_report), Some(next_era_validator_weights)) => {
            Some(EraEnd::new(*era_report, next_era_validator_weights))
        }
        (maybe_era_report, maybe_next_era_validator_weights) => {
            return Err(BlockExecutionError::FailedToCreateEraEnd {
                maybe_era_report,
                maybe_next_era_validator_weights,
            })
        }
    };

    let block = Arc::new(Block::new(
        parent_hash,
        parent_seed,
        state_root_hash,
        finalized_block.random_bit,
        era_end,
        finalized_block.timestamp,
        finalized_block.era_id,
        finalized_block.height,
        protocol_version,
        proposer,
        finalized_block.deploy_hashes,
        finalized_block.transfer_hashes,
    ));

    let approvals_hashes = deploy_ids
        .into_iter()
        .map(|id| id.destructure().1)
        .collect();
    let proof_of_checksum_registry = engine_state.get_checksum_registry_proof(state_root_hash)?;
    let approvals_hashes = Box::new(ApprovalsHashes::new(
        block.hash(),
        approvals_hashes,
        proof_of_checksum_registry,
    ));

    Ok(BlockAndExecutionResults {
        block,
        approvals_hashes,
        execution_results,
        maybe_step_effect_and_upcoming_era_validators,
    })
}

/// Commits the execution results.
fn commit_execution_results<S>(
    engine_state: &EngineState<S>,
    metrics: Option<Arc<Metrics>>,
    state_root_hash: Digest,
    deploy_hash: DeployHash,
    execution_results: ExecutionResults,
) -> Result<(Digest, ExecutionResult), BlockExecutionError>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    let ee_execution_result = execution_results
        .into_iter()
        .exactly_one()
        .map_err(|_| BlockExecutionError::MoreThanOneExecutionResult)?;
    let json_execution_result = ExecutionResult::from(&ee_execution_result);

    let execution_effect: AdditiveMap<Key, Transform> = match ee_execution_result {
        EngineExecutionResult::Success {
            execution_journal,
            cost,
            ..
        } => {
            // We do want to see the deploy hash and cost in the logs.
            // We don't need to see the effects in the logs.
            debug!(?deploy_hash, %cost, "execution succeeded");
            execution_journal
        }
        EngineExecutionResult::Failure {
            error,
            execution_journal,
            cost,
            ..
        } => {
            // Failure to execute a contract is a user error, not a system error.
            // We do want to see the deploy hash, error, and cost in the logs.
            // We don't need to see the effects in the logs.
            debug!(?deploy_hash, ?error, %cost, "execution failure");
            execution_journal
        }
    }
    .into();
    let new_state_root =
        commit_transforms(engine_state, metrics, state_root_hash, execution_effect)?;
    Ok((new_state_root, json_execution_result))
}

fn commit_transforms<S>(
    engine_state: &EngineState<S>,
    metrics: Option<Arc<Metrics>>,
    state_root_hash: Digest,
    effects: AdditiveMap<Key, Transform>,
) -> Result<Digest, engine_state::Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    trace!(?state_root_hash, ?effects, "commit");
    let start = Instant::now();
    let result = engine_state.commit_effects(state_root_hash, effects);
    if let Some(metrics) = metrics {
        metrics.apply_effect.observe(start.elapsed().as_secs_f64());
    }
    trace!(?result, "commit result");
    result.map(Digest::from)
}

/// Execute the transaction without commiting the effects.
/// Intended to be used for discovery operations on read-only nodes.
///
/// Returns effects of the execution.
pub fn execute_only<S>(
    engine_state: &EngineState<S>,
    execution_state: SpeculativeExecutionState,
    deploy: DeployItem,
) -> Result<Option<ExecutionResult>, engine_state::Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    let SpeculativeExecutionState {
        state_root_hash,
        block_time,
        protocol_version,
    } = execution_state;
    let deploy_hash = deploy.deploy_hash;
    let execute_request = ExecuteRequest::new(
        state_root_hash,
        block_time.millis(),
        vec![deploy],
        protocol_version,
        PublicKey::System,
    );
    let results = execute(engine_state, None, execute_request);
    results.map(|mut execution_results| {
        let len = execution_results.len();
        if len != 1 {
            warn!(
                ?deploy_hash,
                "got more ({}) execution results from a single transaction", len
            );
            None
        } else {
            // We know it must be 1, we could unwrap and then wrap
            // with `Some(_)` but `pop_front` already returns an `Option`.
            // We need to transform the `engine_state::ExecutionResult` into
            // `casper_types::ExecutionResult` as well.
            execution_results.pop_front().map(Into::into)
        }
    })
}

fn execute<S>(
    engine_state: &EngineState<S>,
    metrics: Option<Arc<Metrics>>,
    execute_request: ExecuteRequest,
) -> Result<ExecutionResults, engine_state::Error>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    trace!(?execute_request, "execute");
    let start = Instant::now();
    let result = engine_state.run_execute(execute_request);
    if let Some(metrics) = metrics {
        metrics.run_execute.observe(start.elapsed().as_secs_f64());
    }
    trace!(?result, "execute result");
    result
}

fn commit_step<S>(
    engine_state: &EngineState<S>,
    maybe_metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    pre_state_root_hash: Digest,
    era_report: &EraReport<PublicKey>,
    era_end_timestamp_millis: u64,
    next_era_id: EraId,
) -> Result<StepSuccess, StepError>
where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
{
    // Both inactive validators and equivocators are evicted
    let evict_items = era_report
        .inactive_validators()
        .iter()
        .chain(era_report.equivocators())
        .cloned()
        .map(EvictItem::new)
        .collect();

    let step_request = StepRequest {
        pre_state_hash: pre_state_root_hash,
        protocol_version,
        // Note: The Casper Network does not slash, but another network could
        slash_items: vec![],
        evict_items,
        next_era_id,
        era_end_timestamp_millis,
    };

    // Have the EE commit the step.
    let start = Instant::now();
    let result = engine_state.commit_step(step_request);
    if let Some(metrics) = maybe_metrics {
        let elapsed = start.elapsed().as_secs_f64();
        metrics.commit_step.observe(elapsed);
        metrics.latest_commit_step.set(elapsed);
    }
    trace!(?result, "step response");
    result
}

/// Computes the root hash for a Merkle tree constructed from the hashes of execution results.
///
/// NOTE: We're hashing vector of execution results, instead of just their hashes, b/c when a joiner
/// node receives the chunks of *full data* it has to be able to verify it against the Merkle root.
fn compute_execution_results_checksum(
    execution_results: &Vec<ExecutionResult>,
) -> Result<Digest, BlockExecutionError> {
    execution_results
        .hash()
        .map_err(BlockExecutionError::FailedToComputeExecutionResultsChecksum)
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
