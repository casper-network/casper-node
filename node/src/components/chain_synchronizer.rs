mod config;
mod error;
mod event;
mod metrics;
mod operations;
mod progress;

use std::{convert::Infallible, fmt::Debug, marker::PhantomData, sync::Arc};

use datasize::DataSize;
use prometheus::Registry;
use tracing::{debug, error, info};

use casper_execution_engine::storage::trie::TrieOrChunk;

use crate::{
    components::{chainspec_loader::ImmediateSwitchBlockData, Component},
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainSynchronizerAnnouncement, ControlAnnouncement,
        },
        requests::{
            ChainspecLoaderRequest, ContractRuntimeRequest, FetcherRequest,
            MarkBlockCompletedRequest, NetworkInfoRequest, NodeStateRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    storage::StorageRequest,
    types::{
        Block, BlockAndDeploys, BlockHeader, BlockHeaderWithMetadata, BlockHeadersBatch,
        BlockSignatures, BlockWithMetadata, Chainspec, Deploy, FinalizedApprovalsWithId,
        NodeConfig, NodeState,
    },
    NodeRng, SmallNetworkConfig,
};
use config::Config;
pub(crate) use error::Error;
pub(crate) use event::Event;
pub(crate) use metrics::Metrics;
pub(crate) use operations::KeyBlockInfo;
pub(crate) use progress::Progress;
use progress::ProgressHolder;

#[derive(DataSize, Debug)]
pub(crate) enum JoiningOutcome {
    /// We need to shutdown for upgrade as we downloaded a block from a higher protocol version.
    ShouldExitForUpgrade,
    /// We finished initial synchronizing, with the given block header being the result of the fast
    /// sync task.
    Synced {
        highest_block_header: Box<BlockHeader>,
        maybe_immediate_switch_block_data: Option<Box<ImmediateSwitchBlockData>>,
    },
}

#[derive(DataSize, Debug)]
pub(crate) struct ChainSynchronizer<REv> {
    config: Config,
    /// This will be populated once the synchronizer has completed all work, indicating the joiner
    /// reactor can stop running.  It is passed to the participating reactor's constructor via its
    /// config. The participating reactor may still use the chain synchronizer component to run a
    /// sync to genesis in the background.
    joining_outcome: Option<JoiningOutcome>,
    /// Metrics for the chain synchronization process.
    metrics: Metrics,
    /// Records the ongoing progress of chain synchronization.
    progress: ProgressHolder,
    /// The current state of operation of the node.
    node_state: NodeState,
    /// Association with the reactor event used in subtasks.
    _phantom: PhantomData<REv>,
}

impl<REv> ChainSynchronizer<REv>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockWithMetadata>>
        + From<FetcherRequest<BlockHeaderWithMetadata>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalizedApprovalsWithId>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<BlocklistAnnouncement>
        + From<ControlAnnouncement>
        + From<MarkBlockCompletedRequest>
        + Send,
{
    /// Constructs a new `ChainSynchronizer` suitable for use in the joiner reactor to perform the
    /// initial fast sync.
    pub(crate) fn new_for_fast_sync(
        chainspec: Arc<Chainspec>,
        maybe_immediate_switch_block_data: Option<&ImmediateSwitchBlockData>,
        node_config: NodeConfig,
        small_network_config: SmallNetworkConfig,
        effect_builder: EffectBuilder<REv>,
        registry: &Registry,
    ) -> Result<(Self, Effects<Event>), Error> {
        let config = Config::new(chainspec, node_config, small_network_config);
        let metrics = Metrics::new(registry)?;
        let progress = ProgressHolder::new_fast_sync();
        let node_state = NodeState::Joining(progress.progress());

        let maybe_immediate_switch_block_data =
            maybe_immediate_switch_block_data.map(|data| Box::new(data.clone()));

        let effects = operations::run_fast_sync_task(
            effect_builder,
            config.clone(),
            metrics.clone(),
            progress.clone(),
        )
        .event(|result| Event::FastSyncResult {
            result: Box::new(result),
            maybe_immediate_switch_block_data,
        });

        let synchronizer = ChainSynchronizer {
            config,
            joining_outcome: None,
            metrics,
            progress,
            node_state,
            _phantom: PhantomData,
        };

        Ok((synchronizer, effects))
    }

    pub(crate) fn metrics(&self) -> Metrics {
        self.metrics.clone()
    }

    pub(crate) fn joining_outcome(&self) -> Option<&JoiningOutcome> {
        self.joining_outcome.as_ref()
    }

    pub(crate) fn into_joining_outcome(self) -> Option<JoiningOutcome> {
        self.joining_outcome
    }

    fn handle_fast_sync_result(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        result: Result<BlockHeader, Error>,
        maybe_immediate_switch_block_data: Option<Box<ImmediateSwitchBlockData>>,
    ) -> Effects<Event> {
        self.progress.finish();
        match result {
            Ok(highest_block_header) => {
                self.joining_outcome = Some(JoiningOutcome::Synced {
                    highest_block_header: Box::new(highest_block_header),
                    maybe_immediate_switch_block_data,
                });
                Effects::new()
            }
            Err(Error::RetrievedBlockHeaderFromFutureVersion {
                current_version,
                block_header_with_future_version,
            }) => {
                let future_version = block_header_with_future_version.protocol_version();
                info!(%current_version, %future_version, "shutting down for upgrade");
                self.joining_outcome = Some(JoiningOutcome::ShouldExitForUpgrade);
                Effects::new()
            }
            Err(error) => {
                error!(%error, "failed to sync linear chain");
                fatal!(effect_builder, "{}", error).ignore()
            }
        }
    }
}

impl<REv> ChainSynchronizer<REv>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<FetcherRequest<TrieOrChunk>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockSignatures>>
        + From<FetcherRequest<BlockHeadersBatch>>
        + From<ContractRuntimeRequest>
        + From<BlocklistAnnouncement>
        + From<MarkBlockCompletedRequest>
        + From<ChainSynchronizerAnnouncement>
        + Send,
{
    /// Constructs a new `ChainSynchronizer` suitable for use in the participating reactor to sync
    /// to genesis.
    pub(crate) fn new_for_sync_to_genesis(
        chainspec: Arc<Chainspec>,
        node_config: NodeConfig,
        small_network_config: SmallNetworkConfig,
        metrics: Metrics,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<(Self, Effects<Event>), Error> {
        let config = Config::new(chainspec, node_config, small_network_config);
        let progress = ProgressHolder::new_sync_to_genesis();

        if config.sync_to_genesis() {
            let node_state = NodeState::ParticipatingAndSyncingToGenesis {
                sync_progress: progress.progress(),
            };

            let synchronizer = ChainSynchronizer {
                config,
                joining_outcome: None,
                metrics,
                progress: progress.clone(),
                node_state,
                _phantom: PhantomData,
            };

            let effects = operations::run_sync_to_genesis_task(
                effect_builder,
                synchronizer.config.clone(),
                synchronizer.metrics.clone(),
                progress,
            )
            .ignore();

            return Ok((synchronizer, effects));
        }

        // If we're not configured to sync-to-genesis, return without doing anything but announcing
        // that the sync process has finished.
        progress.finish();
        let synchronizer = ChainSynchronizer {
            config,
            joining_outcome: None,
            metrics,
            progress,
            node_state: NodeState::Participating,
            _phantom: PhantomData,
        };

        Ok((
            synchronizer,
            effect_builder.announce_finished_chain_syncing().ignore(),
        ))
    }
}

impl<REv> ChainSynchronizer<REv> {
    fn handle_get_node_state_request(&mut self, request: NodeStateRequest) -> Effects<Event> {
        self.node_state = match self.node_state {
            NodeState::Joining(_) => NodeState::Joining(self.progress.progress()),
            NodeState::ParticipatingAndSyncingToGenesis { .. } => {
                let sync_progress = self.progress.progress();
                if sync_progress.is_finished() {
                    NodeState::Participating
                } else {
                    NodeState::ParticipatingAndSyncingToGenesis { sync_progress }
                }
            }
            NodeState::Participating => NodeState::Participating,
        };

        request.0.respond(self.node_state.clone()).ignore()
    }
}

impl<REv> Component<REv> for ChainSynchronizer<REv>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<BlockHeadersBatch>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockWithMetadata>>
        + From<FetcherRequest<BlockHeaderWithMetadata>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalizedApprovalsWithId>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<BlocklistAnnouncement>
        + From<ControlAnnouncement>
        + From<MarkBlockCompletedRequest>
        + Send,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::FastSyncResult {
                result,
                maybe_immediate_switch_block_data,
            } => self.handle_fast_sync_result(
                effect_builder,
                *result,
                maybe_immediate_switch_block_data,
            ),
            Event::GetNodeState(request) => self.handle_get_node_state_request(request),
        }
    }
}
