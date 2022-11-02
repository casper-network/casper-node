use std::{collections::BTreeMap, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, trace, warn};

use casper_execution_engine::core::engine_state::{
    self, step::EvictItem, DeployItem, EngineState, ExecuteRequest,
    ExecutionResult as EngineExecutionResult, GetEraValidatorsRequest, RewardItem, StepError,
    StepRequest, StepSuccess,
};
use casper_storage::{
    data_access_layer::DataAccessLayer,
    global_state::{
        shared::{transform::Transform, AdditiveMap, CorrelationId},
        storage::state::{CommitProvider, StateProvider},
    },
};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::ToBytes, CLValue, DeployHash, EraId, ExecutionResult, Key, ProtocolVersion,
    PublicKey, U512,
};

use crate::{
    components::{
        consensus::EraReport,
        contract_runtime::{
            error::BlockExecutionError, types::StepEffectAndUpcomingEraValidators,
            BlockAndExecutionEffects, ExecutionPreState, Metrics,
        },
    },
    types::{error::BlockCreationError, Block, Deploy, DeployHeader, FinalizedBlock},
};
use casper_execution_engine::core::{engine_state::execution_result::ExecutionResults, execution};

use super::SpeculativeExecutionState;

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    engine_state: &EngineState<DataAccessLayer>,
    metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    execution_pre_state: ExecutionPreState,
    finalized_block: FinalizedBlock,
    deploys: Vec<Deploy>,
    transfers: Vec<Deploy>,
) -> Result<BlockAndExecutionEffects, BlockExecutionError> {
    if finalized_block.height() != execution_pre_state.next_block_height {
        return Err(BlockExecutionError::WrongBlockHeight {
            finalized_block: Box::new(finalized_block),
            execution_pre_state: Box::new(execution_pre_state),
        });
    }
    let ExecutionPreState {
        pre_state_root_hash,
        parent_hash,
        parent_seed,
        next_block_height: _,
    } = execution_pre_state;
    let mut state_root_hash = pre_state_root_hash;
    let mut execution_results: Vec<(_, DeployHeader, ExecutionResult)> =
        Vec::with_capacity(deploys.len() + transfers.len());
    // Run any deploys that must be executed
    let block_time = finalized_block.timestamp().millis();
    let start = Instant::now();
    let maybe_deploy_approvals_root_hash = compute_approvals_root_hash(&deploys, &transfers)?;

    for deploy in deploys.into_iter().chain(transfers) {
        let deploy_hash = *deploy.id();
        let deploy_header = deploy.header().clone();
        let execute_request = ExecuteRequest::new(
            state_root_hash,
            block_time,
            vec![DeployItem::from(deploy)],
            protocol_version,
            *finalized_block.proposer(),
        );

        // TODO: this is currently working coincidentally because we are passing only one
        // deploy_item per exec. The execution results coming back from the EE lack the
        // mapping between deploy_hash and execution result, and this outer logic is
        // enriching it with the deploy hash. If we were passing multiple deploys per exec
        // the relation between the deploy and the execution results would be lost.
        let result = execute(engine_state, metrics.clone(), execute_request)?;

        trace!(?deploy_hash, ?result, "deploy execution result");
        // As for now a given state is expected to exist.
        let (state_hash, execution_result) = commit_execution_effects(
            engine_state,
            metrics.clone(),
            state_root_hash,
            deploy_hash.into(),
            result,
        )?;
        execution_results.push((deploy_hash, deploy_header, execution_result));
        state_root_hash = state_hash;
    }

    // Write the deploy approvals and execution results Merkle root hashes to global state if there
    // were any deploys.
    let block_height = finalized_block.height();
    if let Some(deploy_approvals_root_hash) = maybe_deploy_approvals_root_hash {
        let execution_results_root_hash = compute_execution_results_root_hash(
            &mut execution_results.iter().map(|(_, _, result)| result),
        )?;

        let mut effects = AdditiveMap::new();
        let _ = effects.insert(
            Key::DeployApprovalsRootHash { block_height },
            Transform::Write(
                CLValue::from_t(deploy_approvals_root_hash)
                    .map_err(BlockCreationError::CLValue)?
                    .into(),
            ),
        );
        let _ = effects.insert(
            Key::BlockEffectsRootHash { block_height },
            Transform::Write(
                CLValue::from_t(execution_results_root_hash)
                    .map_err(BlockCreationError::CLValue)?
                    .into(),
            ),
        );
        engine_state.apply_effect(CorrelationId::new(), state_root_hash, effects)?;
    }

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // If the finalized block has an era report, run the auction contract and get the upcoming era
    // validators.
    let maybe_step_effect_and_upcoming_era_validators =
        if let Some(era_report) = finalized_block.era_report() {
            let StepSuccess {
                post_state_hash: _, // ignore the post-state-hash returned from scratch
                execution_journal: step_execution_journal,
            } = commit_step(
                engine_state, // engine_state
                metrics.clone(),
                protocol_version,
                state_root_hash,
                era_report,
                finalized_block.timestamp().millis(),
                finalized_block.era_id().successor(),
            )?;

            state_root_hash = engine_state.commit_to_disk(state_root_hash)?;

            // In this flow we execute using a recent state root hash where the system contract
            // registry is guaranteed to exist.
            let system_contract_registry = None;

            let upcoming_era_validators = engine_state.get_era_validators(
                CorrelationId::new(),
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
            state_root_hash = engine_state.commit_to_disk(state_root_hash)?;
            None
        };

    // Update the metric.
    if let Some(metrics) = metrics.as_ref() {
        metrics.chain_height.set(block_height as i64);
    }

    let next_era_validator_weights: Option<BTreeMap<PublicKey, U512>> =
        maybe_step_effect_and_upcoming_era_validators
            .as_ref()
            .and_then(
                |StepEffectAndUpcomingEraValidators {
                     upcoming_era_validators,
                     ..
                 }| {
                    upcoming_era_validators
                        .get(&finalized_block.era_id().successor())
                        .cloned()
                },
            );
    let block = Box::new(Block::new(
        parent_hash,
        parent_seed,
        state_root_hash,
        finalized_block,
        next_era_validator_weights,
        protocol_version,
    )?);

    Ok(BlockAndExecutionEffects {
        block,
        execution_results,
        maybe_step_effect_and_upcoming_era_validators,
    })
}

/// Commits the execution effects.
fn commit_execution_effects<S>(
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
    let correlation_id = CorrelationId::new();
    let start = Instant::now();
    let result = engine_state.apply_effect(correlation_id, state_root_hash, effects);
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
    let correlation_id = CorrelationId::new();
    let start = Instant::now();
    let result = engine_state.run_execute(correlation_id, execute_request);
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
    // Extract the rewards and the inactive validators if this is a switch block
    let EraReport {
        equivocators,
        rewards,
        inactive_validators,
    } = era_report;

    let reward_items = rewards
        .iter()
        .map(|(vid, value)| RewardItem::new(vid.clone(), *value))
        .collect();

    // Both inactive validators and equivocators are evicted
    let evict_items = inactive_validators
        .iter()
        .chain(equivocators)
        .cloned()
        .map(EvictItem::new)
        .collect();

    let step_request = StepRequest {
        pre_state_hash: pre_state_root_hash,
        protocol_version,
        reward_items,
        // Note: The Casper Network does not slash, but another network could
        slash_items: vec![],
        evict_items,
        next_era_id,
        era_end_timestamp_millis,
    };

    // Have the EE commit the step.
    let correlation_id = CorrelationId::new();
    let start = Instant::now();
    let result = engine_state.commit_step(correlation_id, step_request);
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
/// node receives the chunks of *full data* it has to be able to verify it against the merkle root.
fn compute_execution_results_root_hash<'a>(
    results: &mut impl Iterator<Item = &'a ExecutionResult>,
) -> Result<Digest, BlockCreationError> {
    let execution_results_bytes = results
        .cloned()
        .collect::<Vec<_>>()
        .to_bytes()
        .map_err(BlockCreationError::BytesRepr)?;
    Ok(Digest::hash_bytes_into_chunks_if_necessary(
        &execution_results_bytes,
    ))
}

/// Returns the computed root hash for a Merkle tree constructed from the hashes of deploy
/// approvals if the combined set of deploys is non-empty, or `None` if the set is empty.
fn compute_approvals_root_hash(
    deploys: &[Deploy],
    transfers: &[Deploy],
) -> Result<Option<Digest>, BlockCreationError> {
    let mut approval_hashes = vec![];
    for deploy in deploys.iter().chain(transfers) {
        let bytes = deploy
            .approvals()
            .to_bytes()
            .map_err(BlockCreationError::BytesRepr)?;
        approval_hashes.push(Digest::hash(bytes));
    }
    Ok((!approval_hashes.is_empty()).then(|| Digest::hash_merkle_tree(approval_hashes)))
}
