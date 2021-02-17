//! Block executor component.
mod event;
mod metrics;

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    convert::Infallible,
    fmt::Debug,
};

use datasize::DataSize;
use itertools::Itertools;
use prometheus::Registry;
use semver::Version;
use smallvec::SmallVec;
use tracing::{debug, error, trace};

use casper_execution_engine::{
    core::engine_state::{
        deploy_item::DeployItem,
        execute_request::ExecuteRequest,
        execution_result::{ExecutionResult as EngineExecutionResult, ExecutionResults},
        step::{EvictItem, RewardItem, SlashItem, StepRequest, StepResult},
    },
    storage::global_state::CommitResult,
};
use casper_types::{ExecutionResult, ProtocolVersion, PublicKey, SemVer, U512};

use crate::{
    components::{
        block_executor::{event::State, metrics::BlockExecutorMetrics},
        Component,
    },
    crypto::hash::Digest,
    effect::{
        announcements::BlockExecutorAnnouncement,
        requests::{
            BlockExecutorRequest, ConsensusRequest, ContractRuntimeRequest, LinearChainRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    types::{
        Block, BlockHash, BlockLike, Deploy, DeployHash, DeployHeader, FinalizedBlock, NodeId,
    },
    NodeRng,
};
pub(crate) use event::Event;

/// A helper trait whose bounds represent the requirements for a reactor event that `BlockExecutor`
/// can work with.
pub trait ReactorEventT:
    From<Event>
    + From<StorageRequest>
    + From<LinearChainRequest<NodeId>>
    + From<ContractRuntimeRequest>
    + From<BlockExecutorAnnouncement>
    + From<ConsensusRequest>
    + Send
{
}

impl<REv> ReactorEventT for REv where
    REv: From<Event>
        + From<StorageRequest>
        + From<LinearChainRequest<NodeId>>
        + From<ContractRuntimeRequest>
        + From<BlockExecutorAnnouncement>
        + From<ConsensusRequest>
        + Send
{
}

#[derive(DataSize, Debug)]
struct ExecutedBlockSummary {
    hash: BlockHash,
    state_root_hash: Digest,
    accumulated_seed: Digest,
}

type BlockHeight = u64;

/// The Block executor component.
#[derive(DataSize, Debug, Default)]
pub(crate) struct BlockExecutor {
    genesis_state_root_hash: Option<Digest>,
    #[data_size(skip)]
    protocol_version: ProtocolVersion,
    /// A mapping from proto block to executed block's ID and post-state hash, to allow
    /// identification of a parent block's details once a finalized block has been executed.
    ///
    /// The key is a tuple of block's height (it's a linear chain so it's monotonically
    /// increasing), and the `ExecutedBlockSummary` is derived from the executed block which is
    /// created from that proto block.
    parent_map: HashMap<BlockHeight, ExecutedBlockSummary>,
    /// Finalized blocks waiting for their pre-state hash to start executing.
    exec_queue: HashMap<BlockHeight, (FinalizedBlock, VecDeque<Deploy>)>,
    /// Metrics to track current chain height.
    #[data_size(skip)]
    metrics: BlockExecutorMetrics,
}

impl BlockExecutor {
    pub(crate) fn new(
        genesis_state_root_hash: Option<Digest>,
        protocol_version: Version,
        registry: Registry,
    ) -> Self {
        let metrics = BlockExecutorMetrics::new(registry).unwrap();
        BlockExecutor {
            genesis_state_root_hash,
            protocol_version: ProtocolVersion::from_parts(
                protocol_version.major as u32,
                protocol_version.minor as u32,
                protocol_version.patch as u32,
            ),
            parent_map: HashMap::new(),
            exec_queue: HashMap::new(),
            metrics,
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
                        state_root_hash: *block.state_root_hash(),
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
        let deploy_hashes = finalized_block
            .proto_block()
            .deploys()
            .iter()
            .map(|hash| **hash)
            .collect::<SmallVec<_>>();
        if deploy_hashes.is_empty() {
            let result_event = move |_| Event::GetDeploysResult {
                finalized_block,
                deploys: VecDeque::new(),
            };
            return effect_builder.immediately().event(result_event);
        }

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
                    .map(|maybe_deploy| {
                        maybe_deploy.unwrap_or_else(|| {
                            panic!(
                                "deploy for block in era={} and height={} is expected to exist \
                                in the storage",
                                era_id, height
                            )
                        })
                    })
                    .collect(),
            })
    }

    /// Creates and announces the linear chain block.
    fn finalize_block_execution<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        state: Box<State>,
        next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
    ) -> Effects<Event> {
        // The state hash of the last execute-commit cycle is used as the block's post state
        // hash.
        let next_height = state.finalized_block.height() + 1;
        // Update the metric.
        self.metrics
            .chain_height
            .set(state.finalized_block.height() as i64);
        let block = self.create_block(
            state.finalized_block,
            state.state_root_hash,
            next_era_validator_weights,
        );

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
                let era_end = match state.finalized_block.era_report() {
                    Some(era_end) => era_end,
                    // Not at a switch block, so we don't need to have next_era_validators when
                    // constructing the next block
                    None => return self.finalize_block_execution(effect_builder, state, None),
                };
                let reward_items = era_end
                    .rewards
                    .iter()
                    .map(|(&vid, &value)| RewardItem::new(vid, value))
                    .collect();
                let slash_items = era_end
                    .equivocators
                    .iter()
                    .map(|&vid| SlashItem::new(vid))
                    .collect();
                let evict_items = era_end
                    .inactive_validators
                    .iter()
                    .map(|&vid| EvictItem::new(vid))
                    .collect();
                let era_end_timestamp_millis = state.finalized_block.timestamp().millis();
                let request = StepRequest {
                    pre_state_hash: state.state_root_hash.into(),
                    protocol_version: self.protocol_version,
                    reward_items,
                    slash_items,
                    evict_items,
                    run_auction: true,
                    next_era_id: state.finalized_block.era_id().successor().into(),
                    era_end_timestamp_millis,
                };
                return effect_builder
                    .run_step(request)
                    .event(|result| Event::RunStepResult { state, result });
            }
        };
        let deploy_hash = *next_deploy.id();
        let deploy_header = next_deploy.header().clone();
        let deploy_item = DeployItem::from(next_deploy);

        let execute_request = ExecuteRequest::new(
            state.state_root_hash.into(),
            state.finalized_block.timestamp().millis(),
            vec![Ok(deploy_item)],
            self.protocol_version,
            state.finalized_block.proposer(),
        );

        // TODO: this is currently working coincidentally because we are passing only one
        // deploy_item per exec. The execution results coming back from the ee lacks the
        // mapping between deploy_hash and execution result, and this outer logic is enriching it
        // with the deploy hash. If we were passing multiple deploys per exec the relation between
        // the deploy and the execution results would be lost.
        effect_builder
            .request_execute(execute_request)
            .event(move |result| Event::DeployExecutionResult {
                state,
                deploy_hash,
                deploy_header,
                result,
            })
    }

    fn handle_get_deploys_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
        deploys: VecDeque<Deploy>,
    ) -> Effects<Event> {
        if let Some(state_root_hash) = self.pre_state_hash(&finalized_block) {
            let state = Box::new(State {
                finalized_block,
                remaining_deploys: deploys,
                execution_results: HashMap::new(),
                state_root_hash,
            });
            self.execute_next_deploy_or_create_block(effect_builder, state)
        } else {
            // Didn't find parent in the `parent_map` cache.
            // Read it from the storage.
            let height = finalized_block.height();
            effect_builder
                .get_block_at_height_local(height - 1)
                .event(|parent| Event::GetParentResult {
                    finalized_block,
                    deploys,
                    parent: parent.map(|b| {
                        (
                            *b.hash(),
                            b.header().accumulated_seed(),
                            *b.state_root_hash(),
                        )
                    }),
                })
        }
    }

    fn handle_get_parent_result<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finalized_block: FinalizedBlock,
        deploys: VecDeque<Deploy>,
        parent: Option<ExecutedBlockSummary>,
    ) -> Effects<Event> {
        match parent {
            None => {
                let height = finalized_block.height();
                debug!("no pre-state hash for height {}", height);
                // re-check the parent map - the parent might have been executed in the meantime!
                if let Some(state_root_hash) = self.pre_state_hash(&finalized_block) {
                    let state = Box::new(State {
                        finalized_block,
                        remaining_deploys: deploys,
                        execution_results: HashMap::new(),
                        state_root_hash,
                    });
                    self.execute_next_deploy_or_create_block(effect_builder, state)
                } else {
                    // The parent block has not been executed yet; delay handling.
                    self.exec_queue.insert(height, (finalized_block, deploys));
                    Effects::new()
                }
            }
            Some(parent_summary) => {
                // Parent found in the storage.
                // Insert into `parent_map` cache.
                // It will be removed in `create_block` method.
                self.parent_map
                    .insert(finalized_block.height().saturating_sub(1), parent_summary);
                self.handle_get_deploys_result(effect_builder, finalized_block, deploys)
            }
        }
    }

    /// Commits the execution effects.
    fn commit_execution_effects<REv: ReactorEventT>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut state: Box<State>,
        deploy_hash: DeployHash,
        deploy_header: DeployHeader,
        execution_results: ExecutionResults,
    ) -> Effects<Event> {
        let ee_execution_result = execution_results
            .into_iter()
            .exactly_one()
            .expect("should only be one exec result");
        let execution_result = ExecutionResult::from(&ee_execution_result);
        let _ = state
            .execution_results
            .insert(deploy_hash, (deploy_header, execution_result));

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
        effect_builder
            .request_commit(state.state_root_hash, execution_effect.transforms)
            .event(|commit_result| Event::CommitExecutionEffects {
                state,
                commit_result,
            })
    }

    fn create_block(
        &mut self,
        finalized_block: FinalizedBlock,
        state_root_hash: Digest,
        next_era_validator_weights: Option<BTreeMap<PublicKey, U512>>,
    ) -> Block {
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
            state_root_hash,
            finalized_block,
            next_era_validator_weights,
            ProtocolVersion::new(SemVer::new(1, 0, 0)), // TODO: Fix
        );
        let summary = ExecutedBlockSummary {
            hash: *block.hash(),
            state_root_hash,
            accumulated_seed: block.header().accumulated_seed(),
        };
        let _ = self.parent_map.insert(block_height, summary);
        block
    }

    fn pre_state_hash(&mut self, finalized_block: &FinalizedBlock) -> Option<Digest> {
        if finalized_block.is_genesis_child() {
            self.genesis_state_root_hash
        } else {
            // Try to get the parent's post-state-hash from the `parent_map`.
            // We're subtracting 1 from the height as we want to get _parent's_ post-state hash.
            let parent_block_height = finalized_block.height() - 1;
            self.parent_map
                .get(&parent_block_height)
                .map(|summary| summary.state_root_hash)
        }
    }
}

impl<REv: ReactorEventT> Component<REv> for BlockExecutor {
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockExecutorRequest::ExecuteBlock(finalized_block)) => {
                debug!(?finalized_block, "execute block");
                effect_builder
                    .get_block_at_height_local(finalized_block.height())
                    .event(move |maybe_block| {
                        maybe_block.map(Box::new).map_or_else(
                            || Event::BlockIsNew(finalized_block),
                            Event::BlockAlreadyExists,
                        )
                    })
            }
            Event::BlockAlreadyExists(block) => {
                effect_builder.handle_linear_chain_block(*block).ignore()
            }
            // If we haven't executed the block before in the past (for example during
            // joining), do it now.
            Event::BlockIsNew(finalized_block) => self.get_deploys(effect_builder, finalized_block),
            Event::GetDeploysResult {
                finalized_block,
                deploys,
            } => {
                trace!(total = %deploys.len(), ?deploys, "fetched deploys");
                self.handle_get_deploys_result(effect_builder, finalized_block, deploys)
            }

            Event::GetParentResult {
                finalized_block,
                deploys,
                parent,
            } => {
                trace!(parent_found = %parent.is_some(), finalized_height = %finalized_block.height(), "fetched parent");
                let parent_summary =
                    parent.map(
                        |(hash, accumulated_seed, state_root_hash)| ExecutedBlockSummary {
                            hash,
                            state_root_hash,
                            accumulated_seed,
                        },
                    );
                self.handle_get_parent_result(
                    effect_builder,
                    finalized_block,
                    deploys,
                    parent_summary,
                )
            }

            Event::DeployExecutionResult {
                state,
                deploy_hash,
                deploy_header,
                result,
            } => {
                trace!(?state, %deploy_hash, ?result, "deploy execution result");
                // As for now a given state is expected to exist.
                let execution_results = result.unwrap();
                self.commit_execution_effects(
                    effect_builder,
                    state,
                    deploy_hash,
                    deploy_header,
                    execution_results,
                )
            }

            Event::CommitExecutionEffects {
                mut state,
                commit_result,
            } => {
                trace!(?state, ?commit_result, "commit result");
                match commit_result {
                    Ok(CommitResult::Success { state_root }) => {
                        debug!(?state_root, "commit succeeded");
                        state.state_root_hash = state_root.into();
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
                    Ok(StepResult::Success {
                        post_state_hash,
                        next_era_validators,
                    }) => {
                        state.state_root_hash = post_state_hash.into();
                        self.finalize_block_execution(
                            effect_builder,
                            state,
                            Some(next_era_validators),
                        )
                    }
                    _ => {
                        // When step fails, the auction process is broken and we should panic.
                        error!(?result, "run step failed - internal contract runtime error");
                        panic!("unable to run step");
                    }
                }
            }
        }
    }
}
