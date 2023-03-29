use std::{cmp, collections::BTreeMap, ops::Range, sync::Arc, time::Instant};

use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::{
    core::engine_state::{
        self,
        purge::{PurgeConfig, PurgeResult},
        step::EvictItem,
        DeployItem, EngineState, ExecuteRequest, ExecutionResult as EngineExecutionResult,
        GetEraValidatorsRequest, RewardItem, StepError, StepRequest, StepSuccess,
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
            BlockAndExecutionEffects, ExecutionPreState, Metrics,
        },
    },
    types::{ActivationPoint, Block, Deploy, DeployHeader, FinalizedBlock},
};
use casper_execution_engine::{
    core::{engine_state::execution_result::ExecutionResults, execution},
    storage::global_state::{CommitProvider, StateProvider},
};

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
enum PruneErasError {
    #[error(
        "time went backwards (current era {current_era_id} < activation point {activation_point})"
    )]
    TimeWentBackwards {
        activation_point: EraId,
        current_era_id: EraId,
    },
    #[error("overflow error while calculating start")]
    StartOverflow,
    #[error("overflow error while calculating end")]
    EndOverflow,
    #[error("last era info id underflow error")]
    LastEraIdUnderflow,
    #[error("underflow error while calculating eras since activation")]
    ErasSinceActivationUnderflow,
}

/// Calculates era keys to be pruned.
///
/// Outcomes:
/// * Ok(Some(range)) -- these ranges should be pruned
/// * Ok(None) -- no more eras should be pruned
/// * Err(PruneErasError{..}) -- precondition error, and current era is lower than activation point
fn calculate_prune_eras(
    activation_point: ActivationPoint,
    current_era_id: EraId,
    batch_size: u64,
) -> Result<Option<Range<u64>>, PruneErasError> {
    let activation_point = match activation_point {
        ActivationPoint::EraId(era_id) if era_id > EraId::new(0) => era_id,
        ActivationPoint::EraId(_) | ActivationPoint::Genesis(_) => {
            // We just created genesis block and there is nothing in the global state to be pruned
            // (yet) Block height 0 is assumed to be synonymous to a genesis block.
            return Ok(None);
        }
    };

    if current_era_id < activation_point {
        return Err(PruneErasError::TimeWentBackwards {
            activation_point,
            current_era_id,
        });
    }

    let current_era_id = current_era_id.value();

    let last_era_info_id = activation_point
        .checked_sub(1)
        .map(EraId::value)
        .ok_or(PruneErasError::LastEraIdUnderflow)?; // one era before activation point
    let eras_since_activation = current_era_id
        .checked_sub(last_era_info_id)
        .ok_or(PruneErasError::ErasSinceActivationUnderflow)?;

    let start = eras_since_activation
        .saturating_sub(1)
        .checked_mul(batch_size)
        .ok_or(PruneErasError::StartOverflow)?;

    let end = cmp::min(
        eras_since_activation
            .checked_mul(batch_size)
            .ok_or(PruneErasError::EndOverflow)?,
        last_era_info_id,
    );

    if start > end {
        return Ok(None);
    }

    Ok(Some(start..end))
}

/// Executes a finalized block.
#[allow(clippy::too_many_arguments)]
pub fn execute_finalized_block(
    engine_state: &EngineState<LmdbGlobalState>,
    metrics: Option<Arc<Metrics>>,
    protocol_version: ProtocolVersion,
    execution_pre_state: ExecutionPreState,
    finalized_block: FinalizedBlock,
    deploys: Vec<Deploy>,
    transfers: Vec<Deploy>,
    activation_point: ActivationPoint,
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

    // Create a new EngineState that reads from LMDB but only caches changes in memory.
    let scratch_state = engine_state.get_scratch_engine_state();

    for deploy in deploys.into_iter().chain(transfers) {
        let deploy_hash = *deploy.id();
        let deploy_header = deploy.header().clone();
        let execute_request = ExecuteRequest::new(
            state_root_hash,
            block_time,
            vec![DeployItem::from(deploy)],
            protocol_version,
            finalized_block.proposer().clone(),
        );

        // TODO: this is currently working coincidentally because we are passing only one
        // deploy_item per exec. The execution results coming back from the ee lacks the
        // mapping between deploy_hash and execution result, and this outer logic is
        // enriching it with the deploy hash. If we were passing multiple deploys per exec
        // the relation between the deploy and the execution results would be lost.
        let result = execute(&scratch_state, metrics.clone(), execute_request)?;

        trace!(?deploy_hash, ?result, "deploy execution result");
        // As for now a given state is expected to exist.
        let (state_hash, execution_result) = commit_execution_effects(
            &scratch_state,
            metrics.clone(),
            state_root_hash,
            deploy_hash.into(),
            result,
        )?;
        execution_results.push((deploy_hash, deploy_header, execution_result));
        state_root_hash = state_hash;
    }

    if let Some(metrics) = metrics.as_ref() {
        metrics.exec_block.observe(start.elapsed().as_secs_f64());
    }

    // If the finalized block has an era report, run the auction contract and get the upcoming era
    // validators
    let maybe_step_effect_and_upcoming_era_validators =
        if let Some(era_report) = finalized_block.era_report() {
            let StepSuccess {
                post_state_hash: _, // ignore the post-state-hash returned from scratch
                execution_journal: step_execution_journal,
            } = commit_step(
                &scratch_state, // engine_state
                metrics.clone(),
                protocol_version,
                state_root_hash,
                era_report,
                finalized_block.timestamp().millis(),
                finalized_block.era_id().successor(),
            )?;

            state_root_hash =
                engine_state.write_scratch_to_db(state_root_hash, scratch_state.into_inner())?;

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
            state_root_hash =
                engine_state.write_scratch_to_db(state_root_hash, scratch_state.into_inner())?;
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

    // Purging

    match calculate_prune_eras(activation_point, finalized_block.era_id(), 100) {
        Ok(Some(era_range)) => {
            let keys_to_purge: Vec<Key> = era_range.map(EraId::new).map(Key::EraInfo).collect();
            let first_key = keys_to_purge.first().copied();
            let last_key = keys_to_purge.last().copied();
            info!(finalized_block_era_id=%finalized_block.era_id(), %activation_point, %state_root_hash, first_key=?first_key, last_key=?last_key, "prune: nothing to do");
            let purge_config = PurgeConfig::new(state_root_hash, keys_to_purge);
            match engine_state.commit_purge(CorrelationId::new(), purge_config) {
                Ok(PurgeResult::RootNotFound) => {
                    error!(finalized_block_era_id=%finalized_block.era_id(), %state_root_hash, "purge: root not found");
                }
                Ok(PurgeResult::DoesNotExist) => {
                    warn!(finalized_block_era_id=%finalized_block.era_id(), %state_root_hash, "purge: key does not exist");
                }
                Ok(PurgeResult::Success { post_state_hash }) => {
                    info!(finalized_block_era_id=%finalized_block.era_id(), %activation_point, %state_root_hash, %post_state_hash, first_key=?first_key, last_key=?last_key, "prune: nothing to do");
                    state_root_hash = post_state_hash;
                }
                Err(error) => {
                    error!(finalized_block_era_id=%finalized_block.era_id(), %activation_point, %error, "purge: commit purge error");
                    return Err(error.into());
                }
            }
        }
        Ok(None) => {
            info!(finalized_block_era_id=finalized_block.era_id().value(), %activation_point, "purge: nothing to do");
        }
        Err(error) => {
            error!(finalized_block_era_id=finalized_block.era_id().value(), %activation_point, %error, "purge: error while calculating keys to prune");
            return Err(BlockExecutionError::Other(
                "Error while calculating keys to prune".into(),
            ));
        }
    }

    let block = Block::new(
        parent_hash,
        parent_seed,
        state_root_hash,
        finalized_block,
        next_era_validator_weights,
        protocol_version,
    )?;

    let patched_execution_results = execution_results
        .into_iter()
        .map(|(hash, header, result)| (hash, (header, result)))
        .collect();

    Ok(BlockAndExecutionEffects {
        block,
        execution_results: patched_execution_results,
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::types::Timestamp;

    use super::*;

    #[test]
    fn should_calculate_prune_eras() {
        const ACTIVATION_POINT_ERA_ID: EraId = EraId::new(6061);

        const ACTIVATION_POINT: ActivationPoint = ActivationPoint::EraId(ACTIVATION_POINT_ERA_ID);
        const CURRENT_ERA_ID: EraId = ACTIVATION_POINT_ERA_ID;
        const BATCH_SIZE: u64 = 100;

        assert_eq!(
            calculate_prune_eras(
                ActivationPoint::Genesis(Timestamp::from_str("2021-03-31T15:00:00Z").unwrap()),
                ACTIVATION_POINT_ERA_ID - 1,
                BATCH_SIZE
            ),
            Ok(None),
            "Genesis block does not need to prune anything"
        );

        assert_eq!(
            calculate_prune_eras(
                ActivationPoint::EraId(EraId::new(0)),
                EraId::new(0),
                BATCH_SIZE
            ),
            Ok(None),
            "Genesis block does not need to prune anything"
        );

        assert_eq!(
            calculate_prune_eras(ACTIVATION_POINT, ACTIVATION_POINT_ERA_ID - 1, BATCH_SIZE),
            Err(PruneErasError::TimeWentBackwards {
                activation_point: ACTIVATION_POINT_ERA_ID,
                current_era_id: ACTIVATION_POINT_ERA_ID - 1,
            })
        );

        let vec: Vec<_> = (0..10)
            .map(|offset| {
                calculate_prune_eras(ACTIVATION_POINT, CURRENT_ERA_ID + offset, BATCH_SIZE)
            })
            .collect();
        assert_eq!(vec[0], Ok(Some(0..100)));
        assert_eq!(vec[1], Ok(Some(100..200)));
        assert_eq!(vec[2], Ok(Some(200..300)));

        let range = calculate_prune_eras(ACTIVATION_POINT, CURRENT_ERA_ID + 60, BATCH_SIZE);
        assert_eq!(range, Ok(Some(6000..6060)));

        let range = calculate_prune_eras(ACTIVATION_POINT, CURRENT_ERA_ID + 61, BATCH_SIZE);
        assert_eq!(range, Ok(None));

        let range = calculate_prune_eras(
            ActivationPoint::EraId(EraId::new(1)),
            EraId::new(u64::MAX),
            BATCH_SIZE,
        );
        assert_eq!(range, Err(PruneErasError::StartOverflow));
    }
}
