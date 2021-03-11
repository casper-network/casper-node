use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
};

use derive_more::From;

use casper_execution_engine::{
    core::{
        engine_state,
        engine_state::{step::StepResult, ExecutionResults},
    },
    storage::global_state::CommitResult,
};
use casper_types::ExecutionResult;

use crate::{
    crypto::hash::Digest,
    effect::requests::BlockExecutorRequest,
    types::{Block, BlockHash, Deploy, DeployHash, DeployHeader, FinalizedBlock},
};

/// Block executor component event.
#[derive(Debug, From)]
pub enum Event {
    /// Indicates that block has already been finalized and executed in the past.
    BlockAlreadyExists(Box<Block>),
    /// Indicates that a block is not known yet, and needs to be executed.
    BlockIsNew(FinalizedBlock),
    /// A request made of the Block executor component.
    #[from]
    Request(BlockExecutorRequest),
    /// Received all requested deploys.
    GetDeploysResult {
        /// The block that needs the deploys for execution.
        finalized_block: FinalizedBlock,
        /// Contents of deploys. All deploys are expected to be present in the storage component.
        deploys: VecDeque<Deploy>,
    },
    GetParentResult {
        /// The block that needs the deploys for execution.
        finalized_block: FinalizedBlock,
        /// Contents of deploys. All deploys are expected to be present in the storage component.
        deploys: VecDeque<Deploy>,
        /// Parent of the newly finalized block.
        /// If it's the first block after Genesis then `parent` is `None`.
        parent: Option<(BlockHash, Digest, Digest)>,
    },
    /// The result of executing a single deploy.
    DeployExecutionResult {
        /// State of this request.
        state: Box<State>,
        /// The ID of the deploy currently being executed.
        deploy_hash: DeployHash,
        /// The header of the deploy currently being executed.
        deploy_header: DeployHeader,
        /// Result of deploy execution.
        result: Result<ExecutionResults, engine_state::Error>,
    },
    /// The result of committing a single set of transforms after executing a single deploy.
    CommitExecutionEffects {
        /// State of this request.
        state: Box<State>,
        /// Commit result for execution request.
        commit_result: Result<CommitResult, engine_state::Error>,
    },
    /// The result of running the step on a switch block.
    RunStepResult {
        /// State of this request.
        state: Box<State>,
        /// The result.
        result: Result<StepResult, engine_state::Error>,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Request(req) => write!(f, "{}", req),
            Event::GetDeploysResult {
                finalized_block,
                deploys,
            } => write!(
                f,
                "fetch deploys for finalized block with height {} has {} deploys",
                finalized_block.height(),
                deploys.len()
            ),
            Event::GetParentResult {
                finalized_block,
                parent,
                ..
            } => write!(
                f,
                "found_parent={} for finalized block with height {}",
                parent.is_some(),
                finalized_block.height()
            ),
            Event::DeployExecutionResult {
                state,
                deploy_hash,
                result: Ok(_),
                ..
            } => write!(
                f,
                "execution result for {} of finalized block with height {} with \
                pre-state hash {}: success",
                deploy_hash,
                state.finalized_block.height(),
                state.state_root_hash
            ),
            Event::DeployExecutionResult {
                state,
                deploy_hash,
                result: Err(_),
                ..
            } => write!(
                f,
                "execution result for {} of finalized block with height {} with \
                pre-state hash {}: root not found",
                deploy_hash,
                state.finalized_block.height(),
                state.state_root_hash
            ),
            Event::CommitExecutionEffects {
                state,
                commit_result: Ok(CommitResult::Success { state_root, .. }),
            } => write!(
                f,
                "commit execution effects of finalized block with height {} with \
                pre-state hash {}: success with post-state hash {}",
                state.finalized_block.height(),
                state.state_root_hash,
                state_root,
            ),
            Event::CommitExecutionEffects {
                state,
                commit_result,
            } => write!(
                f,
                "commit execution effects of finalized block with height {} with \
                pre-state hash {}: failed {:?}",
                state.finalized_block.height(),
                state.state_root_hash,
                commit_result,
            ),
            Event::RunStepResult { state, result } => write!(
                f,
                "result of running the step after finalized block with height {} \
                    with pre-state hash {}: {:?}",
                state.finalized_block.height(),
                state.state_root_hash,
                result
            ),
            Event::BlockAlreadyExists(block) => {
                write!(f, "Block at height {} was executed before", block.height())
            }
            Event::BlockIsNew(fb) => write!(f, "Block at height {} is new", fb.height(),),
        }
    }
}

/// Holds the state of an ongoing execute-commit cycle spawned from a given `Event::Request`.
#[derive(Debug)]
pub struct State {
    pub finalized_block: FinalizedBlock,
    /// Deploys which have still to be executed.
    pub remaining_deploys: VecDeque<Deploy>,
    /// A collection of results of executing the deploys.
    pub execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    /// Current state root hash of global storage.  Is initialized with the parent block's
    /// state hash, and is updated after each commit.
    pub state_root_hash: Digest,
}
