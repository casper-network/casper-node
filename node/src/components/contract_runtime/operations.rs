use std::{collections::BTreeMap, sync::Arc, time::Instant};

use tracing::{debug, trace};

use casper_execution_engine::{
    core::engine_state::{
        self, step::EvictItem, DeployItem, EngineState, ExecuteRequest, Execution,
        ExecutionResult as EngineExecutionResult, GetEraValidatorsRequest, RewardItem, StepError,
        StepRequest, StepSuccess,
    },
    shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform},
    storage::global_state::lmdb::LmdbGlobalState,
};
use casper_hashing::Digest;
use casper_types::{DeployHash, EraId, ExecutionResult, Key, ProtocolVersion, PublicKey, U512};

use crate::{
    components::{
        consensus::EraReport,
        contract_runtime::{
            error::BlockExecutionError, types::StepEffectAndUpcomingEraValidators,
            BlockAndExecutionEffects, ContractRuntimeMetrics, ExecutionPreState,
        },
    },
    types::{Block, Deploy, DeployHash as NodeDeployHash, DeployHeader, FinalizedBlock},
};
use casper_execution_engine::{
    core::execution,
    storage::global_state::{CommitProvider, StateProvider},
};

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    engine_state: &EngineState<LmdbGlobalState>,
    metrics: Option<Arc<ContractRuntimeMetrics>>,
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

    // Run any deploys that must be executed
    let start = Instant::now();

    let (execution_results, mut state_root_hash) = execute_and_commit(
        engine_state,
        metrics.clone(),
        protocol_version,
        &finalized_block,
        transfers.into_iter().chain(deploys.into_iter()).collect(),
        pre_state_root_hash,
    )?;

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // If the finalized block has an era report, run the auction contract and get the upcoming era
    // validators
    let maybe_step_effect_and_upcoming_era_validators =
        if let Some(era_report) = finalized_block.era_report() {
            let StepSuccess {
                post_state_hash,
                execution_journal: step_execution_journal,
            } = commit_step(
                engine_state,
                metrics.clone(),
                protocol_version,
                state_root_hash,
                era_report,
                finalized_block.timestamp().millis(),
                finalized_block.era_id().successor(),
            )?;
            state_root_hash = post_state_hash;
            let upcoming_era_validators = engine_state.get_era_validators(
                CorrelationId::new(),
                GetEraValidatorsRequest::new(state_root_hash, protocol_version),
            )?;
            Some(StepEffectAndUpcomingEraValidators {
                step_execution_journal,
                upcoming_era_validators,
            })
        } else {
            None
        };

    // Flush once, after all deploys have been executed.
    engine_state.flush_environment()?;

    // Update the metric.
    let block_height = finalized_block.height();
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
    let block = Block::new(
        parent_hash,
        parent_seed,
        state_root_hash,
        finalized_block,
        next_era_validator_weights,
        protocol_version,
    )?;

    Ok(BlockAndExecutionEffects {
        block,
        execution_results: execution_results.into_iter().collect(),
        maybe_step_effect_and_upcoming_era_validators,
    })
}

type ExecResultMap = BTreeMap<NodeDeployHash, (DeployHeader, ExecutionResult)>;

fn execute_and_commit(
    engine_state: &EngineState<LmdbGlobalState>,
    metrics: Option<Arc<ContractRuntimeMetrics>>,
    protocol_version: ProtocolVersion,
    finalized_block: &FinalizedBlock,
    deploys: Vec<Deploy>,
    state_root_hash: Digest,
) -> Result<(ExecResultMap, Digest), BlockExecutionError> {
    let block_time = finalized_block.timestamp().millis();

    // Create a new EngineState that reads from LMDB but only caches changes in memory.
    let scratch_state = engine_state.get_scratch_engine_state();

    let deploy_items = deploys
        .iter()
        .cloned()
        .map(DeployItem::from)
        .collect::<Vec<_>>();

    let mut execution_results = BTreeMap::new();

    for deploy_item in deploy_items {
        // Note that we generate a request to execute all deploys in the block.
        let execute_request = ExecuteRequest::new(
            state_root_hash,
            block_time,
            vec![deploy_item],
            protocol_version,
            finalized_block.proposer(),
        );

        // Execute our request against the in-memory cache.
        let Execution { exec_results } = execute(&scratch_state, metrics.clone(), execute_request)?;

        let transforms_by_deploy = extract_transforms_from_execution_results(&exec_results);

        // This ensures that transforms are applied in the order they were executed.
        for (_deploy_hash, transforms) in transforms_by_deploy.into_iter() {
            // Because we are applying the transforms to the ScratchGlobalState, no new
            // state-root-hash is generated here.
            let _same_root_hash =
                apply_transforms(&scratch_state, metrics.clone(), state_root_hash, transforms)?;
        }

        for (deploy_hash, ee_exec_result) in exec_results {
            let node_deploy_hash: NodeDeployHash = deploy_hash.into();
            if let Some(deploy) = deploys
                .iter()
                .find(|deploy| node_deploy_hash == *deploy.id())
            {
                execution_results.insert(
                    node_deploy_hash,
                    (
                        deploy.header().clone(),
                        ExecutionResult::from(&ee_exec_result),
                    ),
                );
            }
        }
    }

    // Finally the new state-root-hash from the cumulative changes to global state is returned when
    // they are written to LMDB.
    let new_state_root_hash =
        engine_state.write_scratch_to_lmdb(state_root_hash, scratch_state.into_inner())?;

    Ok((execution_results, new_state_root_hash))
}

fn extract_transforms_from_execution_results(
    execution_results: &[(DeployHash, EngineExecutionResult)],
) -> Vec<(DeployHash, AdditiveMap<Key, Transform>)> {
    let mut all_transforms = Vec::new();
    for (deploy_hash, ee_execution_result) in execution_results.iter() {
        let mut per_deploy_transforms = AdditiveMap::new();
        match ee_execution_result {
            EngineExecutionResult::Success { cost, .. } => {
                // We do want to see the deploy hash and cost in the logs.
                // We don't need to see the effects in the logs.
                debug!(?deploy_hash, %cost, "execution succeeded");
            }
            EngineExecutionResult::Failure { error, cost, .. } => {
                // Failure to execute a contract is a user error, not a system error.
                // We do want to see the deploy hash, error, and cost in the logs.
                // We don't need to see the effects in the logs.
                debug!(?deploy_hash, ?error, %cost, "execution failure");
            }
        }
        let journal = ee_execution_result.execution_journal().clone();
        let journal: AdditiveMap<Key, Transform> = journal.into();
        for (key, transform) in journal.iter() {
            per_deploy_transforms.insert(*key, transform.clone());
        }
        all_transforms.push((*deploy_hash, per_deploy_transforms));
    }
    all_transforms
}

fn apply_transforms<S>(
    engine_state: &EngineState<S>,
    metrics: Option<Arc<ContractRuntimeMetrics>>,
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

fn execute<S>(
    engine_state: &EngineState<S>,
    metrics: Option<Arc<ContractRuntimeMetrics>>,
    execute_request: ExecuteRequest,
) -> Result<Execution, engine_state::Error>
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

fn commit_step(
    engine_state: &EngineState<LmdbGlobalState>,
    maybe_metrics: Option<Arc<ContractRuntimeMetrics>>,
    protocol_version: ProtocolVersion,
    pre_state_root_hash: Digest,
    era_report: &EraReport<PublicKey>,
    era_end_timestamp_millis: u64,
    next_era_id: EraId,
) -> Result<StepSuccess, StepError> {
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
        metrics.commit_step.observe(start.elapsed().as_secs_f64());
    }
    trace!(?result, "step response");
    result
}
