//! Block executor component.

use std::fmt::{Debug, Display};

use derive_more::From;
use rand::Rng;
use tracing::{debug, error, trace, warn};

use super::{
    chainspec_handler::ChainspecHandler,
    contract_runtime::{
        core::engine_state::{self, execute_request::ExecuteRequest},
        storage::global_state::CommitResult,
    },
    storage::Storage,
};
use crate::{
    components::Component,
    crypto::hash::Digest,
    effect::{
        requests::{BlockExecutorRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{Deploy, ExecutedBlock, FinalizedBlock, Timestamp},
};
use engine_state::execution_result::{ExecutionResult, ExecutionResults};
use types::ProtocolVersion;

/// The Block executor components.
#[derive(Debug)]
pub(crate) struct BlockExecutor {
    // NOTE: As of today state hash is kept track here without any assumption
    // regarding process restarts etc. This may change in future.
    /// Current post state hash.
    post_state_hash: Digest,
}

/// Block executor component event.
#[derive(Debug, From)]
pub enum Event {
    /// A request made of the Block executor component.
    #[from]
    Request(BlockExecutorRequest),
    /// Received all requested deploys.
    GetDeploysResult {
        /// Finalized block that is passed around from the original request in `Event::Request`.
        finalized_block: FinalizedBlock,
        /// Contents of deploys. All deploys are expected to be present in the storage layer.
        deploys: Vec<Deploy>,
        /// Original responder passed with `Event::Request`.
        main_responder: Responder<ExecutedBlock>,
    },
    /// Contract execution result.
    DeploysExecutionResult {
        /// Finalized block used to request execution on.
        finalized_block: FinalizedBlock,
        /// Result of deploy execution.
        result: Result<ExecutionResults, engine_state::RootNotFound>,
        /// Original responder passed with `Event::Request`.
        main_responder: Responder<ExecutedBlock>,
    },
    /// Commit effects
    CommitExecutionEffects {
        /// Finalized block used to request execution on.
        finalized_block: FinalizedBlock,
        /// Commit result for execution request.
        commit_result: Result<CommitResult, engine_state::Error>,
        /// Results
        results: ExecutionResults,
        /// Original responder passed with `Event::Request`.
        main_responder: Responder<ExecutedBlock>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Request(req) => write!(f, "{}", req),
            Event::GetDeploysResult {
                finalized_block,
                deploys,
                ..
            } => write!(
                f,
                "fetch deploys for block {} has {} deploys",
                finalized_block,
                deploys.len()
            ),
            Event::DeploysExecutionResult {
                finalized_block,
                result: Ok(result),
                ..
            } => write!(
                f,
                "deploys execution result {}, total results: {}",
                finalized_block,
                result.len()
            ),
            Event::DeploysExecutionResult {
                finalized_block,
                result: Err(_),
                ..
            } => write!(
                f,
                "deploys execution result {}, root not found",
                finalized_block
            ),
            Event::CommitExecutionEffects { results, .. } if results.is_empty() => {
                write!(f, "commit execution effects tail")
            }
            Event::CommitExecutionEffects { results, .. } => write!(
                f,
                "commit execution effects remaining {} results",
                results.len()
            ),
        }
    }
}

impl<REv> Component<REv> for BlockExecutor
where
    REv: From<Event> + From<StorageRequest<Storage>> + From<ContractRuntimeRequest> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockExecutorRequest::ExecuteBlock {
                finalized_block,
                responder,
            }) => {
                debug!(?finalized_block, "execute block");

                if finalized_block.0.deploys.is_empty() {
                    // No deploys - short circuit and respond straight away using current state hash.
                    let executed_block = ExecutedBlock {
                        finalized_block,
                        post_state_hash: self.post_state_hash,
                    };
                    return responder.respond(executed_block).ignore();
                }

                let deploy_hashes = finalized_block.0.deploys.clone().into_iter().collect();

                // Get all deploys in order they appear in the finalized block.
                effect_builder
                    .get_deploys_from_storage(deploy_hashes)
                    .event(move |result| Event::GetDeploysResult {
                        finalized_block,
                        deploys: result
                            .into_iter()
                            // Assumes all deploys are present
                            .map(|maybe_deploy| {
                                maybe_deploy.expect("deploy is expected to exist in the storage")
                            })
                            .collect(),
                        main_responder: responder,
                    })
            }

            Event::GetDeploysResult {
                finalized_block,
                deploys,
                main_responder,
            } => {
                trace!(total = %deploys.len(), ?deploys, "fetched deploys");
                let execute_request = self.create_execute_request_from(deploys);
                effect_builder
                    .request_execute(execute_request)
                    .event(move |result| Event::DeploysExecutionResult {
                        finalized_block,
                        result,
                        main_responder,
                    })
            }

            Event::DeploysExecutionResult {
                finalized_block,
                result,
                main_responder,
            } => {
                trace!(?finalized_block, ?result, "deploys execution result");
                match result {
                    Ok(execution_results) => self.process_execution_results(
                        effect_builder,
                        finalized_block,
                        execution_results,
                        main_responder,
                    ),
                    Err(_) => {
                        // NOTE: As for now a given state is expected to exist
                        panic!("root not found");
                    }
                }
            }

            Event::CommitExecutionEffects {
                finalized_block,
                commit_result,
                results,
                main_responder,
            } => {
                match commit_result {
                    Ok(CommitResult::Success {
                        state_root,
                        bonded_validators,
                    }) => {
                        debug!(?state_root, ?bonded_validators, "commit succeeded");
                        // Update current post state hash as this will be used for next commit.
                        self.post_state_hash = state_root;
                    }
                    Ok(result) => warn!(?result, "commit succeeded in unexpected state"),
                    Err(error) => {
                        error!(?error, "commit failed");
                        // When commit fails we panic as well to avoid being out of sync in next block.
                        panic!("unable to commit");
                    }
                }

                self.process_execution_results(
                    effect_builder,
                    finalized_block,
                    results,
                    main_responder,
                )
            }
        }
    }
}

impl From<ChainspecHandler> for BlockExecutor {
    fn from(chainspec_handler: ChainspecHandler) -> Self {
        BlockExecutor::new(chainspec_handler.post_state_hash().unwrap())
    }
}

impl BlockExecutor {
    pub(crate) fn new(post_state_hash: Digest) -> Self {
        BlockExecutor { post_state_hash }
    }

    /// Creates new `ExecuteRequest` from a list of deploys.
    fn create_execute_request_from(&self, deploys: Vec<Deploy>) -> ExecuteRequest {
        let deploy_items = deploys
            .into_iter()
            .map(|deploy| Ok(deploy.into()))
            .collect();

        ExecuteRequest::new(
            self.post_state_hash,
            // TODO: Use `BlockContext`'s timestamp as part of NDRS-175
            Timestamp::now().millis(),
            deploy_items,
            ProtocolVersion::V1_0_0,
        )
    }

    /// Consumes execution results and dispatches appropriate events.
    fn process_execution_results<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
        mut execution_results: ExecutionResults,
        responder: Responder<ExecutedBlock>,
    ) -> Effects<Event>
    where
        REv: From<Event> + From<StorageRequest<Storage>> + From<ContractRuntimeRequest> + Send,
    {
        let effect = match execution_results.pop_front() {
            Some(ExecutionResult::Success { effect, cost }) => {
                debug!(?effect, %cost, "execution succeeded");
                effect
            }
            Some(ExecutionResult::Failure {
                error,
                effect,
                cost,
            }) => {
                error!(?error, ?effect, %cost, "execution failure");
                effect
            }
            None => {
                // We processed all executed deploys.
                let executed_block = ExecutedBlock {
                    finalized_block,
                    post_state_hash: self.post_state_hash,
                };
                trace!(?executed_block, "all execution results processed");
                return responder.respond(executed_block).ignore();
            }
        };

        // There's something more to process.
        effect_builder
            .request_commit(
                ProtocolVersion::V1_0_0,
                self.post_state_hash,
                effect.transforms,
            )
            .event(|commit_result| Event::CommitExecutionEffects {
                finalized_block,
                commit_result,
                results: execution_results,
                main_responder: responder,
            })
    }
}
