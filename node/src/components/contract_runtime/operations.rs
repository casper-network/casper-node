use std::{collections::VecDeque, sync::Arc, time::Instant};

use engine_state::ExecuteRequest;
use itertools::Itertools;
use smallvec::SmallVec;
use tracing::{debug, error, trace};

use casper_execution_engine::{
    core::engine_state::{
        self, EngineState, ExecutionResult as EngineExecutionResult, ExecutionResults,
    },
    shared::{additive_map::AdditiveMap, newtypes::CorrelationId, transform::Transform},
    storage::global_state::{lmdb::LmdbGlobalState, CommitResult},
};
use casper_types::{ExecutionResult, Key};

use crate::{
    components::contract_runtime::{ContractRuntimeMetrics, ContractRuntimeResult, Event},
    crypto::hash::Digest,
    effect::{announcements::ControlAnnouncement, requests::StorageRequest, EffectBuilder},
    fatal,
    types::{Deploy, DeployHash, FinalizedBlock},
};

/// Commits the execution effects.
pub(super) async fn commit_execution_effects(
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<ContractRuntimeMetrics>,
    state_root_hash: Digest,
    deploy_hash: DeployHash,
    execution_results: ExecutionResults,
) -> Result<(Digest, ExecutionResult), ()> {
    let ee_execution_result = execution_results
        .into_iter()
        .exactly_one()
        .expect("should only be one exec result");
    let execution_result = ExecutionResult::from(&ee_execution_result);

    let execution_effect = match ee_execution_result {
        EngineExecutionResult::Success { effect, cost, .. } => {
            // We do want to see the deploy hash and cost in the logs.
            // We don't need to see the effects in the logs.
            debug!(?deploy_hash, %cost, "execution succeeded");
            effect
        }
        EngineExecutionResult::Failure {
            error,
            effect,
            cost,
            ..
        } => {
            // Failure to execute a contract is a user error, not a system error.
            // We do want to see the deploy hash, error, and cost in the logs.
            // We don't need to see the effects in the logs.
            debug!(?deploy_hash, ?error, %cost, "execution failure");
            effect
        }
    };
    let commit_result = commit(
        engine_state,
        metrics,
        state_root_hash,
        execution_effect.transforms,
    )
    .await;
    trace!(?commit_result, "commit result");
    match commit_result {
        Ok(CommitResult::Success { state_root }) => {
            debug!(?state_root, "commit succeeded");
            Ok((state_root.into(), execution_result))
        }
        _ => {
            error!(
                ?commit_result,
                "commit failed - internal contract runtime error"
            );
            Err(())
        }
    }
}

pub(super) async fn commit(
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<ContractRuntimeMetrics>,
    state_root_hash: Digest,
    effects: AdditiveMap<Key, Transform>,
) -> Result<CommitResult, engine_state::Error> {
    trace!(?state_root_hash, ?effects, "commit");
    let correlation_id = CorrelationId::new();
    let start = Instant::now();
    let result = engine_state.apply_effect(correlation_id, state_root_hash.into(), effects);
    metrics.apply_effect.observe(start.elapsed().as_secs_f64());
    trace!(?result, "commit result");
    result
}

pub(super) async fn execute(
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<ContractRuntimeMetrics>,
    execute_request: ExecuteRequest,
) -> Result<VecDeque<EngineExecutionResult>, engine_state::Error> {
    trace!(?execute_request, "execute");
    let correlation_id = CorrelationId::new();
    let start = Instant::now();
    let result = engine_state.run_execute(correlation_id, execute_request);
    metrics.run_execute.observe(start.elapsed().as_secs_f64());
    trace!(?result, "execute result");
    result
}

/// Gets the deploy(s) of the given finalized block from storage.
pub(super) async fn get_deploys_and_finalize_block<REv>(
    effect_builder: EffectBuilder<REv>,
    finalized_block: FinalizedBlock,
) -> Option<Event>
where
    REv: From<ControlAnnouncement> + From<StorageRequest>,
{
    // Get the deploy hashes for the finalized block.
    let deploy_hashes = finalized_block
        .deploys_and_transfers_iter()
        .copied()
        .collect::<SmallVec<_>>();

    let era_id = finalized_block.era_id();
    let height = finalized_block.height();

    // Get all deploys in order they appear in the finalized block.
    let mut deploys: VecDeque<Deploy> = VecDeque::with_capacity(deploy_hashes.len());
    for maybe_deploy in effect_builder.get_deploys_from_storage(deploy_hashes).await {
        if let Some(deploy) = maybe_deploy {
            deploys.push_back(deploy)
        } else {
            fatal!(
                effect_builder,
                "deploy for block in era={} and height={} is expected to exist in the storage",
                era_id,
                height
            )
            .await;
            return None;
        }
    }
    Some(Event::Result(Box::new(
        ContractRuntimeResult::GetDeploysResult {
            finalized_block,
            deploys,
        },
    )))
}
