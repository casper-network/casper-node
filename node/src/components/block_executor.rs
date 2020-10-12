//! Block executor component.
mod event;

use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
};

use datasize::DataSize;
use itertools::Itertools;
use smallvec::SmallVec;
use tracing::{debug, error, trace};

use casper_execution_engine::{
    core::engine_state::{
        deploy_item::DeployItem,
        execute_request::ExecuteRequest,
        execution_result::{ExecutionResult as EngineExecutionResult, ExecutionResults},
        step::{RewardItem, SlashItem, StepRequest, StepResult},
    },
    storage::global_state::CommitResult,
};
use casper_types::ProtocolVersion;

use crate::{
    components::{block_executor::event::State, storage::Storage, Component},
    crypto::hash::Digest,
    effect::{
        announcements::BlockExecutorAnnouncement,
        requests::{BlockExecutorRequest, ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    types::{
        json_compatibility::ExecutionResult, Block, BlockHash, CryptoRngCore, Deploy, DeployHash,
        FinalizedBlock,
    },
};
pub(crate) use event::Event;

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

#[derive(DataSize, Debug)]
struct ExecutedBlockSummary {
    hash: BlockHash,
    post_state_hash: Digest,
    accumulated_seed: Digest,
}

type BlockHeight = u64;

/// The Block executor component.
#[derive(DataSize, Debug, Default)]
pub(crate) struct BlockExecutor {
    genesis_post_state_hash: Digest,
    /// A mapping from proto block to executed block's ID and post-state hash, to allow
    /// identification of a parent block's details once a finalized block has been executed.
    ///
    /// The key is a tuple of block's height (it's a linear chain so it's monotonically
    /// increasing), and the `ExecutedBlockSummary` is derived from the executed block which is
    /// created from that proto block.
    parent_map: HashMap<BlockHeight, ExecutedBlockSummary>,
    /// Finalized blocks waiting for their pre-state hash to start executing.
    exec_queue: HashMap<BlockHeight, (FinalizedBlock, VecDeque<Deploy>)>,
}

impl BlockExecutor {
    pub(crate) fn new(genesis_post_state_hash: Digest) -> Self {
        BlockExecutor {
            genesis_post_state_hash,
            parent_map: HashMap::new(),
            exec_queue: HashMap::new(),
        }
    }

    /// Adds the "parent map" to the instance of `BlockExecutor`.
    ///
    /// When transitioning from `joiner` to `validator` states we need
    /// to carry over the last finalized block so that the next blocks in the linear chain
    /// have the state to build on.
    pub(crate) fn with_parent_map(mut self, lfb: Option<Block>) -> Self {
        let parent_map = lfb
            .into_iter()
            .map(|block| {
                (
                    block.height(),
                    ExecutedBlockSummary {
                        hash: *block.hash(),
                        post_state_hash: *block.global_state_hash(),
                        accumulated_seed: block.header().accumulated_seed(),
                    },
                )
            })
            .collect();
        self.parent_map = parent_map;
        self
    }

    /// Gets the deploy(s) of the given finalized block from storage.
    fn get_deploys<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
    ) -> Effects<Event> {
        let deploy_hashes = SmallVec::from_slice(finalized_block.proto_block().deploys());
        let era_id = finalized_block.era_id();
        let height = finalized_block.height();

        // Get all deploys in order they appear in the finalized block.
        effect_builder
            .get_deploys_from_storage(deploy_hashes)
            .event(move |result| Event::GetDeploysResult {
                finalized_block,
                deploys: result
                    .into_iter()
                    // Assumes all deploys are present
                    .map(|maybe_deploy| maybe_deploy.unwrap_or_else(|| panic!("deploy for block in era={} and height={} is expected to exist in the storage", era_id, height)))
                    .collect(),
            })
    }

    /// Creates and announces the linear chain block.
    fn finalize_block_execution<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        state: Box<State>,
    ) -> Effects<Event> {
        // The state hash of the last execute-commit cycle is used as the block's post state
        // hash.
        let next_height = state.finalized_block.height() + 1;
        let block = self.create_block(state.finalized_block, state.pre_state_hash);

        let mut effects = effect_builder
            .announce_linear_chain_block(block, state.execution_results)
            .ignore();
        // If the child is already finalized, start execution.
        if let Some((finalized_block, deploys)) = self.exec_queue.remove(&next_height) {
            effects.extend(self.handle_get_deploys_result(
                effect_builder,
                finalized_block,
                deploys,
            ));
        }
        effects
    }

    /// Executes the first deploy in `state.remaining_deploys`, or creates the executed block if
    /// there are no remaining deploys left.
    fn execute_next_deploy_or_create_block<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut state: Box<State>,
    ) -> Effects<Event> {
        let next_deploy = match state.remaining_deploys.pop_front() {
            Some(deploy) => deploy,
            None => {
                let era_end = match state.finalized_block.era_end().as_ref() {
                    Some(era_end) => era_end,
                    None => return self.finalize_block_execution(effect_builder, state),
                };
                let reward_items = era_end
                    .rewards
                    .iter()
                    .map(|(&vid, &value)| RewardItem::new(vid.into(), value))
                    .collect();
                let slash_items = era_end
                    .equivocators
                    .iter()
                    .map(|&vid| SlashItem::new(vid.into()))
                    .collect();
                let request = StepRequest {
                    pre_state_hash: state.pre_state_hash.into(),
                    protocol_version: ProtocolVersion::V1_0_0,
                    reward_items,
                    slash_items,
                    run_auction: true,
                };
                return effect_builder
                    .run_step(request)
                    .event(|result| Event::RunStepResult { state, result });
            }
        };
        let deploy_hash = *next_deploy.id();
        let deploy_item = DeployItem::from(next_deploy);

        let execute_request = ExecuteRequest::new(
            state.pre_state_hash.into(),
            state.finalized_block.timestamp().millis(),
            vec![Ok(deploy_item)],
            ProtocolVersion::V1_0_0,
        );

        effect_builder
            .request_execute(execute_request)
            .event(move |result| Event::DeployExecutionResult {
                state,
                deploy_hash,
                result,
            })
    }

    fn handle_get_deploys_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
        deploys: VecDeque<Deploy>,
    ) -> Effects<Event> {
        if let Some(pre_state_hash) = self.pre_state_hash(&finalized_block) {
            let state = Box::new(State {
                finalized_block,
                remaining_deploys: deploys,
                execution_results: HashMap::new(),
                pre_state_hash,
            });
            self.execute_next_deploy_or_create_block(effect_builder, state)
        } else {
            let height = finalized_block.height();
            debug!("No pre-state hash for height {}", height);
            // The parent block has not been executed yet; delay handling.
            self.exec_queue.insert(height, (finalized_block, deploys));
            Effects::new()
        }
    }

    /// Commits the execution effects.
    fn commit_execution_effects<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut state: Box<State>,
        deploy_hash: DeployHash,
        execution_results: ExecutionResults,
    ) -> Effects<Event> {
        let ee_execution_result = execution_results
            .into_iter()
            .exactly_one()
            .expect("should only be one exec result");
        let execution_result = ExecutionResult::from(&ee_execution_result);
        let _ = state
            .execution_results
            .insert(deploy_hash, execution_result);

        let execution_effect = match ee_execution_result {
            EngineExecutionResult::Success { effect, cost } => {
                debug!(?effect, %cost, "execution succeeded");
                effect
            }
            EngineExecutionResult::Failure {
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
        let (parent_summary_hash, parent_seed) = if finalized_block.is_genesis_child() {
            // Genesis, no parent summary.
            (BlockHash::new(Digest::default()), Digest::default())
        } else {
            let parent_block_height = finalized_block.height() - 1;
            let summary = self
                .parent_map
                .remove(&parent_block_height)
                .unwrap_or_else(|| panic!("failed to take {:?}", parent_block_height));
            (summary.hash, summary.accumulated_seed)
        };
        let block_height = finalized_block.height();
        let block = Block::new(
            parent_summary_hash,
            parent_seed,
            post_state_hash,
            finalized_block,
        );
        let summary = ExecutedBlockSummary {
            hash: *block.hash(),
            post_state_hash,
            accumulated_seed: block.header().accumulated_seed(),
        };
        let _ = self.parent_map.insert(block_height, summary);
        block
    }

    fn pre_state_hash(&mut self, finalized_block: &FinalizedBlock) -> Option<Digest> {
        if finalized_block.is_genesis_child() {
            Some(self.genesis_post_state_hash)
        } else {
            // Try to get the parent's post-state-hash from the `parent_map`.
            // We're subtracting 1 from the height as we want to get _parent's_ post-state hash.
            let parent_block_height = finalized_block.height() - 1;
            self.parent_map
                .get(&parent_block_height)
                .map(|summary| summary.post_state_hash)
        }
    }
}

impl<REv: ReactorEventT> Component<REv> for BlockExecutor {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockExecutorRequest::ExecuteBlock(finalized_block)) => {
                debug!(?finalized_block, "execute block");
                if finalized_block.proto_block().deploys().is_empty() {
                    effect_builder
                        .immediately()
                        .event(move |_| Event::GetDeploysResult {
                            finalized_block,
                            deploys: VecDeque::new(),
                        })
                } else {
                    self.get_deploys(effect_builder, finalized_block)
                }
            }

            Event::GetDeploysResult {
                finalized_block,
                deploys,
            } => {
                trace!(total = %deploys.len(), ?deploys, "fetched deploys");
                self.handle_get_deploys_result(effect_builder, finalized_block, deploys)
            }

            Event::DeployExecutionResult {
                state,
                deploy_hash,
                result,
            } => {
                trace!(?state, %deploy_hash, ?result, "deploy execution result");
                // As for now a given state is expected to exist.
                let execution_results = result.unwrap();
                self.commit_execution_effects(effect_builder, state, deploy_hash, execution_results)
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

            Event::RunStepResult { mut state, result } => {
                trace!(?result, "run step result");
                match result {
                    Ok(StepResult::Success { post_state_hash }) => {
                        state.pre_state_hash = post_state_hash.into();
                        self.finalize_block_execution(effect_builder, state)
                    }
                    _ => {
                        error!(?result, "run step failed - internal contract runtime error");
                        panic!("unable to run step");
                    }
                }
            }
        }
    }
}
