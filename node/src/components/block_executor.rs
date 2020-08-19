//! Block executor component.

use std::{
    collections::{HashMap, VecDeque},
    fmt::{Debug, Display},
};

use derive_more::From;
use rand::Rng;
use tracing::{debug, error, trace};

use casperlabs_types::ProtocolVersion;

use crate::{
    components::{
        contract_runtime::{
            core::engine_state::{
                self,
                deploy_item::DeployItem,
                execute_request::ExecuteRequest,
                execution_result::{ExecutionResult, ExecutionResults},
                RootNotFound,
            },
            storage::global_state::CommitResult,
        },
        storage::Storage,
        Component,
    },
    crypto::hash::Digest,
    effect::{
        requests::{BlockExecutorRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{Block, BlockHash, Deploy, FinalizedBlock, ProtoBlockHash},
};

/// A helper trait whose bounds represent the requirements for a reactor event that `BlockExecutor`
/// can work with.
pub trait ReactorEventT:
    From<Event> + From<StorageRequest<Storage>> + From<ContractRuntimeRequest> + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event> + From<StorageRequest<Storage>> + From<ContractRuntimeRequest> + Send
{
}

/// Block executor component event.
#[derive(Debug, From)]
pub enum Event {
    /// A request made of the Block executor component.
    #[from]
    Request(BlockExecutorRequest),
    /// Received all requested deploys.
    GetDeploysResult {
        /// State of this request.
        state: State,
        /// Contents of deploys. All deploys are expected to be present in the storage component.
        deploys: VecDeque<Deploy>,
    },
    /// The result of executing a single deploy.
    DeployExecutionResult {
        /// State of this request.
        state: State,
        /// Result of deploy execution.
        result: Result<ExecutionResults, RootNotFound>,
    },
    /// The result of committing a single set of transforms after executing a single deploy.
    CommitExecutionEffects {
        /// State of this request.
        state: State,
        /// Commit result for execution request.
        commit_result: Result<CommitResult, engine_state::Error>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Request(req) => write!(f, "{}", req),
            Event::GetDeploysResult {
                state,
                deploys,
            } => write!(
                f,
                "fetch deploys for finalized block {} has {} deploys",
                state.finalized_block.proto_block().hash(),
                deploys.len()
            ),
            Event::DeployExecutionResult {
                state,
                result: Ok(_),
            } => write!(
                f,
                "deploys execution result for finalized block {} with pre-state hash {}: success",
                state.finalized_block.proto_block().hash(),
                state.pre_state_hash
            ),
            Event::DeployExecutionResult {
                state,
                result: Err(_),
            } => write!(
                f,
                "deploys execution result for finalized block {} with pre-state hash {}: root not found",
                state.finalized_block.proto_block().hash(),
                state.pre_state_hash
            ),
            Event::CommitExecutionEffects { state, commit_result: Ok(CommitResult::Success { state_root, ..})} => write!(
                f,
                "commit execution effects for finalized block {} with pre-state hash {}: success with post-state hash {}",
                state.finalized_block.proto_block().hash(),
                state.pre_state_hash,
                state_root,
            ),
            Event::CommitExecutionEffects { state, commit_result } => write!(
                f,
                "commit execution effects for finalized block {} with pre-state hash {}: failed {:?}",
                state.finalized_block.proto_block().hash(),
                state.pre_state_hash,
                commit_result,
            ),
        }
    }
}

/// Holds the state of an ongoing execute-commit cycle spawned from a given `Event::Request`.
#[derive(Debug)]
pub struct State {
    finalized_block: FinalizedBlock,
    responder: Responder<Block>,
    /// Deploys which have still to be executed.
    remaining_deploys: VecDeque<Deploy>,
    /// Current pre-state hash of global storage.  Is initialized with the parent block's post-state
    /// hash, and is updated after each commit.
    pre_state_hash: Digest,
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

    /// Gets the deploy(s) of the given finalized block from storage.
    fn get_deploys<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
        responder: Responder<Block>,
    ) -> Effects<Event> {
        let deploy_hashes = finalized_block
            .proto_block()
            .deploys()
            .iter()
            .copied()
            .collect();

        let pre_state_hash = self.pre_state_hash(&finalized_block);
        let state = State {
            finalized_block,
            responder,
            remaining_deploys: VecDeque::new(),
            pre_state_hash,
        };

        // Get all deploys in order they appear in the finalized block.
        effect_builder
            .get_deploys_from_storage(deploy_hashes)
            .event(move |result| Event::GetDeploysResult {
                state,
                deploys: result
                    .into_iter()
                    // Assumes all deploys are present
                    .map(|maybe_deploy| {
                        maybe_deploy.expect("deploy is expected to exist in the storage")
                    })
                    .collect(),
            })
    }

    /// Executes the first deploy in `state.remaining_deploys`.
    fn execute_next_deploy<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut state: State,
    ) -> Effects<Event> {
        let next_deploy = state
            .remaining_deploys
            .pop_front()
            .expect("should not be empty");
        let deploy_item = DeployItem::from(next_deploy);

        let execute_request = ExecuteRequest::new(
            state.pre_state_hash,
            state.finalized_block.timestamp().millis(),
            vec![Ok(deploy_item)],
            ProtocolVersion::V1_0_0,
        );

        effect_builder
            .request_execute(execute_request)
            .event(move |result| Event::DeployExecutionResult { state, result })
    }

    /// Commits the execution effects.
    fn commit_execution_effects<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        state: State,
        mut execution_results: ExecutionResults,
    ) -> Effects<Event> {
        assert_eq!(execution_results.len(), 1, "should only be one exec result");
        let execution_effect = match execution_results.pop_front().unwrap() {
            ExecutionResult::Success { effect, cost } => {
                debug!(?effect, %cost, "execution succeeded");
                effect
            }
            ExecutionResult::Failure {
                error,
                effect,
                cost,
            } => {
                error!(?error, ?effect, %cost, "execution failure");
                effect
            }
        };
        effect_builder
            .request_commit(
                ProtocolVersion::V1_0_0,
                state.pre_state_hash,
                execution_effect.transforms,
            )
            .event(|commit_result| Event::CommitExecutionEffects {
                state,
                commit_result,
            })
    }

    /// Either cycles to the next deploy for execution, or creates the executed block if there are
    /// no further deploys to execute.
    fn process_commit_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut state: State,
        post_state_hash: Digest,
    ) -> Effects<Event> {
        if state.remaining_deploys.is_empty() {
            state.pre_state_hash = post_state_hash;
            self.execute_next_deploy(effect_builder, state)
        } else {
            let block = self.create_block(state.finalized_block, post_state_hash);
            state.responder.respond(block).ignore()
        }
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

        // If the proto block has a default parent hash, its parent is the genesis block.  The
        // default hash is applied in `EffectBuilder::request_proto_block` when the parent proto
        // block is `None`.
        if *finalized_block.proto_block().parent_hash().inner() == Digest::default()
            && self.genesis_post_state_hash.is_some()
        {
            return self.genesis_post_state_hash.take().unwrap();
        }

        error!(%parent_proto_hash, "failed to get pre-state-hash");
        panic!("failed to get pre-state hash for {}", parent_proto_hash);
    }
}

impl<REv: ReactorEventT> Component<REv> for BlockExecutor {
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
                    // TODO - account for executing `Instruction`s in finalized block.  For now,
                    //        just create the block.
                    let post_state_hash = self.pre_state_hash(&finalized_block);
                    let block = self.create_block(finalized_block, post_state_hash);
                    responder.respond(block).ignore()
                } else {
                    self.get_deploys(effect_builder, finalized_block, responder)
                }
            }

            Event::GetDeploysResult { mut state, deploys } => {
                trace!(total = %deploys.len(), ?deploys, "fetched deploys");
                state.remaining_deploys = deploys;
                self.execute_next_deploy(effect_builder, state)
            }

            Event::DeployExecutionResult { state, result } => {
                trace!(?state, ?result, "deploy execution result");
                match result {
                    Ok(execution_results) => {
                        self.commit_execution_effects(effect_builder, state, execution_results)
                    }
                    Err(_) => {
                        // As for now a given state is expected to exist.
                        panic!("root not found");
                    }
                }
            }

            Event::CommitExecutionEffects {
                state,
                commit_result,
            } => {
                trace!(?state, ?commit_result, "commit result");
                match commit_result {
                    Ok(CommitResult::Success {
                        state_root: post_state_hash,
                        bonded_validators,
                    }) => {
                        debug!(?post_state_hash, ?bonded_validators, "commit succeeded");
                        self.process_commit_result(effect_builder, state, post_state_hash)
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
                }
            }
        }
    }
}
