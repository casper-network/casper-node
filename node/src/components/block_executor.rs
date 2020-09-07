//! Block executor component.

use std::{
    collections::{HashMap, VecDeque},
    fmt::{Debug, Display},
};

use derive_more::From;
use itertools::Itertools;
use rand::{CryptoRng, Rng};
use tracing::{debug, error, trace};

use casper_execution_engine::{
    core::engine_state::{
        self,
        deploy_item::DeployItem,
        execute_request::ExecuteRequest,
        execution_result::{ExecutionResult, ExecutionResults},
        RootNotFound,
    },
    storage::global_state::CommitResult,
};
use casper_types::ProtocolVersion;

use crate::{
    components::{storage::Storage, Component},
    crypto::hash::Digest,
    effect::{
        announcements::BlockExecutorAnnouncement,
        requests::{BlockExecutorRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{Block, BlockHash, Deploy, FinalizedBlock},
};

/// A helper trait whose bounds represent the requirements for a reactor event that `BlockExecutor`
/// can work with.
pub trait ReactorEventT:
    From<Event>
    + From<StorageRequest<Storage>>
    + From<ContractRuntimeRequest>
    + From<BlockExecutorAnnouncement>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<StorageRequest<Storage>>
        + From<ContractRuntimeRequest>
        + From<BlockExecutorAnnouncement>
        + Send
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
                "fetch deploys for finalized block with height {} has {} deploys",
                state.finalized_block.height(),
                deploys.len()
            ),
            Event::DeployExecutionResult {
                state,
                result: Ok(_),
            } => write!(
                f,
                "deploys execution result for finalized block with height {} with pre-state hash {}: success",
                state.finalized_block.height(),
                state.pre_state_hash
            ),
            Event::DeployExecutionResult {
                state,
                result: Err(_),
            } => write!(
                f,
                "deploys execution result for finalized block with height {} with pre-state hash {}: root not found",
                state.finalized_block.height(),
                state.pre_state_hash
            ),
            Event::CommitExecutionEffects { state, commit_result: Ok(CommitResult::Success { state_root, ..})} => write!(
                f,
                "commit execution effects for finalized block with height {} with pre-state hash {}: success with post-state hash {}",
                state.finalized_block.height(),
                state.pre_state_hash,
                state_root,
            ),
            Event::CommitExecutionEffects { state, commit_result } => write!(
                f,
                "commit execution effects for finalized block with height {} with pre-state hash {}: failed {:?}",
                state.finalized_block.height(),
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
    /// Deploys which have still to be executed.
    remaining_deploys: VecDeque<Deploy>,
    /// Current pre-state hash of global storage.  Is initialized with the parent block's
    /// post-state hash, and is updated after each commit.
    pre_state_hash: Digest,
}

#[derive(Debug)]
struct ExecutedBlockSummary {
    hash: BlockHash,
    post_state_hash: Digest,
}

type BlockHeight = u64;

/// The Block executor component.
#[derive(Debug, Default)]
pub(crate) struct BlockExecutor {
    genesis_post_state_hash: Digest,
    /// A mapping from proto block to executed block's ID and post-state hash, to allow
    /// identification of a parent block's details once a finalized block has been executed.
    ///
    /// The key is a tuple of block's height (it's a linear chain so it's monotonically
    /// increasing), and the `ExecutedBlockSummary` is derived from the executed block which is
    /// created from that proto block.
    parent_map: HashMap<BlockHeight, ExecutedBlockSummary>,
}

impl BlockExecutor {
    pub(crate) fn new(genesis_post_state_hash: Digest) -> Self {
        BlockExecutor {
            genesis_post_state_hash,
            parent_map: HashMap::new(),
        }
    }

    /// Gets the deploy(s) of the given finalized block from storage.
    fn get_deploys<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
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

    /// Executes the first deploy in `state.remaining_deploys`, or creates the executed block if
    /// there are no remaining deploys left.
    fn execute_next_deploy_or_create_block<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut state: State,
    ) -> Effects<Event> {
        let next_deploy = match state.remaining_deploys.pop_front() {
            Some(deploy) => deploy,
            None => {
                // The state hash of the last execute-commit cycle is used as the block's post state
                // hash.
                let block = self.create_block(state.finalized_block, state.pre_state_hash);
                return effect_builder.announce_linear_chain_block(block).ignore();
            }
        };
        let deploy_item = DeployItem::from(next_deploy);

        let execute_request = ExecuteRequest::new(
            state.pre_state_hash.into(),
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
        execution_results: ExecutionResults,
    ) -> Effects<Event> {
        let execution_effect = match execution_results
            .into_iter()
            .exactly_one()
            .expect("should only be one exec result")
        {
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
            .request_commit(state.pre_state_hash, execution_effect.transforms)
            .event(|commit_result| Event::CommitExecutionEffects {
                state,
                commit_result,
            })
    }

    fn create_block(&mut self, finalized_block: FinalizedBlock, post_state_hash: Digest) -> Block {
        let parent_summary_hash = if finalized_block.is_genesis_child() {
            // Genesis, no parent summary.
            BlockHash::new(Digest::default())
        } else {
            let parent_block_height = finalized_block.height() - 1;
            self.parent_map
                .remove(&parent_block_height)
                .unwrap_or_else(|| panic!("failed to take {:?}", parent_block_height))
                .hash
        };
        let block_height = finalized_block.height();
        let block = Block::new(parent_summary_hash, post_state_hash, finalized_block);
        let summary = ExecutedBlockSummary {
            hash: *block.hash(),
            post_state_hash,
        };
        let _ = self.parent_map.insert(block_height, summary);
        block
    }

    fn pre_state_hash(&mut self, finalized_block: &FinalizedBlock) -> Digest {
        if finalized_block.is_genesis_child() {
            self.genesis_post_state_hash
        } else {
            // Try to get the parent's post-state-hash from the `parent_map`.
            // We're subtracting 1 from the height as we want to get _parent's_ post-state hash.
            let parent_block_height = finalized_block.height() - 1;
            match self
                .parent_map
                .get(&parent_block_height)
                .map(|summary| summary.post_state_hash)
            {
                None => {
                    error!(?parent_block_height, "failed to get pre-state-hash");
                    panic!("failed to get pre-state hash for {:?}", parent_block_height);
                }
                Some(hash) => hash,
            }
        }
    }
}

impl<REv: ReactorEventT, R: Rng + CryptoRng + ?Sized> Component<REv, R> for BlockExecutor {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockExecutorRequest::ExecuteBlock(finalized_block)) => {
                debug!(?finalized_block, "execute block");
                self.get_deploys(effect_builder, finalized_block)
            }

            Event::GetDeploysResult { mut state, deploys } => {
                trace!(total = %deploys.len(), ?deploys, "fetched deploys");
                state.remaining_deploys = deploys;
                self.execute_next_deploy_or_create_block(effect_builder, state)
            }

            Event::DeployExecutionResult { state, result } => {
                trace!(?state, ?result, "deploy execution result");
                // As for now a given state is expected to exist.
                let execution_results = result.unwrap();
                self.commit_execution_effects(effect_builder, state, execution_results)
            }

            Event::CommitExecutionEffects {
                mut state,
                commit_result,
            } => {
                trace!(?state, ?commit_result, "commit result");
                match commit_result {
                    Ok(CommitResult::Success {
                        state_root: post_state_hash,
                    }) => {
                        debug!(?post_state_hash, "commit succeeded");
                        state.pre_state_hash = post_state_hash.into();
                        self.execute_next_deploy_or_create_block(effect_builder, state)
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
