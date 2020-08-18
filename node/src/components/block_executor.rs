//! Block executor component.

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
};

use derive_more::From;
use rand::Rng;
use tracing::{debug, error, trace};

use casperlabs_types::ProtocolVersion;

use super::{
    contract_runtime::{
        core::engine_state::{
            self,
            execute_request::ExecuteRequest,
            execution_result::{ExecutionResult, ExecutionResults},
            RootNotFound,
        },
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
    types::{Block, BlockHash, Deploy, FinalizedBlock, ProtoBlockHash, Timestamp},
};

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
        main_responder: Responder<Block>,
    },
    /// Contract execution result.
    DeploysExecutionResult {
        /// Finalized block used to request execution on.
        finalized_block: FinalizedBlock,
        /// Result of deploy execution.
        result: Result<ExecutionResults, RootNotFound>,
        /// Original responder passed with `Event::Request`.
        main_responder: Responder<Block>,
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
        main_responder: Responder<Block>,
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

#[derive(Debug)]
struct ExecutedBlockSummary {
    hash: BlockHash,
    post_state_hash: Digest,
}

/// The Block executor component.
#[derive(Debug, Default)]
pub(crate) struct BlockExecutor {
    genesis_post_state_hash: Option<Digest>,
    /// A mapping from proto block to executed block's ID and post-state hash, to allow
    /// identification of a parent block's details once a finalized block has been executed.
    ///
    /// For a given entry, the key is a proto block's hash, and the `ExecutedBlockSummary` is
    /// derived from the executed block which is created from that proto block.
    parent_map: HashMap<ProtoBlockHash, ExecutedBlockSummary>,
}

impl BlockExecutor {
    pub(crate) fn new(genesis_post_state_hash: Digest) -> Self {
        BlockExecutor {
            genesis_post_state_hash: Some(genesis_post_state_hash),
            parent_map: HashMap::new(),
        }
    }

    /// Creates new `ExecuteRequest` from a list of deploys.
    fn create_execute_request_from(
        &self,
        pre_state_hash: Digest,
        timestamp: &Timestamp,
        deploys: Vec<Deploy>,
    ) -> ExecuteRequest {
        let deploy_items = deploys
            .into_iter()
            .map(|deploy| Ok(deploy.into()))
            .collect();

        ExecuteRequest::new(
            pre_state_hash,
            timestamp.millis(),
            deploy_items,
            ProtocolVersion::V1_0_0,
        )
    }

    /// Consumes execution results and dispatches appropriate events.
    fn process_execution_results<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
        maybe_post_state_hash: Option<Digest>,
        mut execution_results: ExecutionResults,
        responder: Responder<Block>,
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
                let post_state_hash =
                    maybe_post_state_hash.unwrap_or_else(|| self.pre_state_hash(&finalized_block));
                let block = self.create_block(finalized_block, post_state_hash);
                trace!(?block, "all execution results processed");
                return responder.respond(block).ignore();
            }
        };

        // There's something more to process.
        let pre_state_hash = self.pre_state_hash(&finalized_block);
        effect_builder
            .request_commit(ProtocolVersion::V1_0_0, pre_state_hash, effect.transforms)
            .event(|commit_result| Event::CommitExecutionEffects {
                finalized_block,
                commit_result,
                results: execution_results,
                main_responder: responder,
            })
    }

    fn create_block(&mut self, finalized_block: FinalizedBlock, post_state_hash: Digest) -> Block {
        let proto_parent_hash = finalized_block.proto_block().parent_hash();
        let parent_summary = self
            .parent_map
            .remove(proto_parent_hash)
            .unwrap_or_else(|| panic!("failed to take {}", proto_parent_hash));
        let new_proto_hash = *finalized_block.proto_block().hash();
        let block = Block::new(parent_summary.hash, post_state_hash, finalized_block);
        let summary = ExecutedBlockSummary {
            hash: *block.hash(),
            post_state_hash,
        };
        let _ = self.parent_map.insert(new_proto_hash, summary);
        block
    }

    fn pre_state_hash(&mut self, finalized_block: &FinalizedBlock) -> Digest {
        // Try to get the parent's post-state-hash from the `parent_map`.
        let parent_proto_hash = finalized_block.proto_block().parent_hash();
        if let Some(hash) = self
            .parent_map
            .get(parent_proto_hash)
            .map(|summary| summary.post_state_hash)
        {
            return hash;
        }

        // If the proto block has a default parent hash (indicating it has no parent) assume its
        // parent is the genesis block.
        if *finalized_block.proto_block().parent_hash().inner() == Digest::default()
            && self.genesis_post_state_hash.is_some()
        {
            return self.genesis_post_state_hash.take().unwrap();
        }

        error!(%parent_proto_hash, "failed to get pre-state-hash");
        panic!("failed to get pre-state hash for {}", parent_proto_hash);
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

                if finalized_block.proto_block().deploys().is_empty() {
                    // No deploys - jump straight to execution stage.
                    let pre_state_hash = self.pre_state_hash(&finalized_block);
                    let execute_request = self.create_execute_request_from(
                        pre_state_hash,
                        finalized_block.timestamp(),
                        vec![],
                    );
                    return effect_builder
                        .request_execute(execute_request)
                        .event(move |result| Event::DeploysExecutionResult {
                            finalized_block,
                            result,
                            main_responder: responder,
                        });
                }

                let deploy_hashes = finalized_block
                    .proto_block()
                    .deploys()
                    .clone()
                    .into_iter()
                    .collect();

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
                let pre_state_hash = self.pre_state_hash(&finalized_block);
                let execute_request = self.create_execute_request_from(
                    pre_state_hash,
                    finalized_block.timestamp(),
                    deploys,
                );
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
                        None,
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
                let post_state_hash = match commit_result {
                    Ok(CommitResult::Success {
                        state_root,
                        bonded_validators,
                    }) => {
                        debug!(?state_root, ?bonded_validators, "commit succeeded");
                        state_root
                    }
                    _ => {
                        // When commit fails we panic as we'll not be able to execute the next
                        // block.
                        error!(
                            ?commit_result,
                            "commit failed - internal contract runtime error"
                        );
                        panic!("unable to commit");
                    }
                };

                self.process_execution_results(
                    effect_builder,
                    finalized_block,
                    Some(post_state_hash),
                    results,
                    main_responder,
                )
            }
        }
    }
}
