mod block_acquisition;
mod block_acquisition_action;
mod block_builder;
mod block_synchronizer_progress;
mod config;
mod deploy_acquisition;
mod error;
mod event;
mod execution_results_acquisition;
mod global_state_synchronizer;
mod metrics;
mod need_next;
mod peer_list;
mod signature_acquisition;
mod trie_accumulator;

use datasize::DataSize;
use either::Either;
use once_cell::sync::Lazy;
use prometheus::Registry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::core::engine_state;
use casper_hashing::Digest;
use casper_types::Timestamp;

use super::network::blocklist::BlocklistJustification;
use crate::{
    components::{
        fetcher::{Error as FetcherError, FetchResult, FetchedData},
        Component, ComponentStatus, InitializedComponent, ValidatorBoundComponent,
    },
    effect::{
        announcements::{BlockSynchronizerAnnouncement, PeerBehaviorAnnouncement},
        requests::{
            BlockAccumulatorRequest, BlockCompleteConfirmationRequest, BlockSynchronizerRequest,
            ContractRuntimeRequest, FetcherRequest, MakeBlockExecutableRequest, NetworkInfoRequest,
            StorageRequest, SyncGlobalStateRequest, TrieAccumulatorRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::{self},
    rpcs::docs::DocExample,
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash, BlockHeader,
        BlockSignatures, Deploy, EmptyValidationMetadata, FinalitySignature, FinalitySignatureId,
        Item, LegacyDeploy, NodeId, SyncLeap, TrieOrChunk, ValidatorMatrix,
    },
    NodeRng,
};

use block_builder::BlockBuilder;
pub(crate) use block_synchronizer_progress::BlockSynchronizerProgress;
pub(crate) use config::Config;
pub(crate) use error::BlockAcquisitionError;
pub(crate) use event::Event;
use execution_results_acquisition::ExecutionResultsAcquisition;
pub(crate) use execution_results_acquisition::ExecutionResultsChecksum;
use global_state_synchronizer::GlobalStateSynchronizer;
pub(crate) use global_state_synchronizer::{
    Error as GlobalStateSynchronizerError, Event as GlobalStateSynchronizerEvent,
};
pub(crate) use need_next::NeedNext;
use trie_accumulator::TrieAccumulator;
pub(crate) use trie_accumulator::{Error as TrieAccumulatorError, Event as TrieAccumulatorEvent};

static BLOCK_SYNCHRONIZER_STATUS: Lazy<BlockSynchronizerStatus> = Lazy::new(|| {
    BlockSynchronizerStatus::new(
        Some(BlockSyncStatus {
            block_hash: BlockHash::new(
                Digest::from_hex(
                    "16ddf28e2b3d2e17f4cef36f8b58827eca917af225d139b0c77df3b4a67dc55e",
                )
                .unwrap(),
            ),
            block_height: Some(40),
            acquisition_state: "have strict finality(40) for: block hash 16dd..c55e".to_string(),
        }),
        Some(BlockSyncStatus {
            block_hash: BlockHash::new(
                Digest::from_hex(
                    "59907b1e32a9158169c4d89d9ce5ac9164fc31240bfcfb0969227ece06d74983",
                )
                .unwrap(),
            ),
            block_height: Some(6701),
            acquisition_state: "have block body(6701) for: block hash 5990..4983".to_string(),
        }),
    )
});
use metrics::Metrics;

pub(crate) trait ReactorEvent:
    From<FetcherRequest<ApprovalsHashes>>
    + From<NetworkInfoRequest>
    + From<FetcherRequest<Block>>
    + From<FetcherRequest<BlockHeader>>
    + From<FetcherRequest<LegacyDeploy>>
    + From<FetcherRequest<Deploy>>
    + From<FetcherRequest<FinalitySignature>>
    + From<FetcherRequest<TrieOrChunk>>
    + From<FetcherRequest<BlockExecutionResultsOrChunk>>
    + From<BlockAccumulatorRequest>
    + From<PeerBehaviorAnnouncement>
    + From<StorageRequest>
    + From<TrieAccumulatorRequest>
    + From<ContractRuntimeRequest>
    + From<SyncGlobalStateRequest>
    + From<BlockCompleteConfirmationRequest>
    + From<MakeBlockExecutableRequest>
    + From<BlockSynchronizerAnnouncement>
    + Send
    + 'static
{
}

impl<REv> ReactorEvent for REv where
    REv: From<FetcherRequest<ApprovalsHashes>>
        + From<NetworkInfoRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<LegacyDeploy>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalitySignature>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<FetcherRequest<BlockExecutionResultsOrChunk>>
        + From<BlockAccumulatorRequest>
        + From<PeerBehaviorAnnouncement>
        + From<StorageRequest>
        + From<TrieAccumulatorRequest>
        + From<ContractRuntimeRequest>
        + From<SyncGlobalStateRequest>
        + From<BlockCompleteConfirmationRequest>
        + From<MakeBlockExecutableRequest>
        + From<BlockSynchronizerAnnouncement>
        + Send
        + 'static
{
}

/// The status of syncing an individual block.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BlockSyncStatus {
    /// The block hash.
    block_hash: BlockHash,
    /// The height of the block, if known.
    block_height: Option<u64>,
    /// The state of acquisition of the data associated with the block.
    acquisition_state: String,
}

/// The status of the block synchronizer.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockSynchronizerStatus {
    /// The status of syncing a historical block, if any.
    historical: Option<BlockSyncStatus>,
    /// The status of syncing a forward block, if any.
    forward: Option<BlockSyncStatus>,
}

impl BlockSynchronizerStatus {
    pub(crate) fn new(
        historical: Option<BlockSyncStatus>,
        forward: Option<BlockSyncStatus>,
    ) -> Self {
        Self {
            historical,
            forward,
        }
    }
}

impl DocExample for BlockSynchronizerStatus {
    fn doc_example() -> &'static Self {
        &*BLOCK_SYNCHRONIZER_STATUS
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockSynchronizer {
    status: ComponentStatus,
    config: Config,
    max_simultaneous_peers: u32,
    validator_matrix: ValidatorMatrix,

    // execute forward block (do not get global state or execution effects)
    forward: Option<BlockBuilder>,
    // either sync-to-genesis or sync-leaped block (get global state and execution effects)
    historical: Option<BlockBuilder>,
    // deals with global state acquisition for historical blocks
    global_sync: GlobalStateSynchronizer,
    #[data_size(skip)]
    metrics: Metrics,
}

impl BlockSynchronizer {
    pub(crate) fn new(
        config: Config,
        max_simultaneous_peers: u32,
        validator_matrix: ValidatorMatrix,
        registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(BlockSynchronizer {
            status: ComponentStatus::Uninitialized,
            config,
            max_simultaneous_peers,
            validator_matrix,
            forward: None,
            historical: None,
            global_sync: GlobalStateSynchronizer::new(config.max_parallel_trie_fetches() as usize),
            metrics: Metrics::new(registry)?,
        })
    }

    /// Returns the progress being made on the historical syncing.
    pub(crate) fn historical_progress(&mut self) -> BlockSynchronizerProgress {
        match &self.historical {
            None => BlockSynchronizerProgress::Idle,
            Some(builder) => self.progress(builder),
        }
    }

    /// Returns the progress being made on the forward syncing.
    pub(crate) fn forward_progress(&mut self) -> BlockSynchronizerProgress {
        match &self.forward {
            None => BlockSynchronizerProgress::Idle,
            Some(builder) => self.progress(builder),
        }
    }

    pub(crate) fn purge(&mut self) {
        self.purge_historical();
        self.purge_forward();
    }

    pub(crate) fn purge_historical(&mut self) {
        if let Some(builder) = &self.historical {
            debug!(%builder, "BlockSynchronizer: purging block builder");
        }
        self.historical = None;
    }

    pub(crate) fn purge_forward(&mut self) {
        if let Some(builder) = &self.forward {
            debug!(%builder, "BlockSynchronizer: purging block builder");
        }
        self.forward = None;
    }

    /// Registers a block for synchronization.
    pub(crate) fn register_block_by_hash(
        &mut self,
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        requires_strict_finality: bool,
    ) -> bool {
        if let (true, Some(builder), _) | (false, _, Some(builder)) = (
            should_fetch_execution_state,
            &self.historical,
            &self.forward,
        ) {
            if builder.block_hash() == block_hash && !builder.is_failed() {
                return false;
            }
        }
        let builder = BlockBuilder::new(
            block_hash,
            should_fetch_execution_state,
            requires_strict_finality,
            self.max_simultaneous_peers,
            self.config.peer_refresh_interval(),
        );
        if should_fetch_execution_state {
            self.historical.replace(builder);
        } else {
            self.forward.replace(builder);
        }
        true
    }

    /// Registers a sync leap result, if able.
    pub(crate) fn register_sync_leap(
        &mut self,
        sync_leap: &SyncLeap,
        peers: Vec<NodeId>,
        should_fetch_execution_state: bool,
    ) {
        fn apply_sigs(builder: &mut BlockBuilder, maybe_sigs: Option<&BlockSignatures>) {
            if let Some(signatures) = maybe_sigs {
                for finality_signature in signatures.finality_signatures() {
                    if let Err(error) =
                        builder.register_finality_signature(finality_signature, None)
                    {
                        debug!(%error, "BlockSynchronizer: failed to register finality signature");
                    }
                }
            }
        }

        let (block_header, maybe_sigs) = sync_leap.highest_block_header();
        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder))
                if builder.block_hash() == block_header.block_hash() =>
            {
                apply_sigs(builder, maybe_sigs);
                builder.register_peers(peers);
            }
            _ => {
                let era_id = block_header.era_id();
                if let Some(validator_weights) = self.validator_matrix.validator_weights(era_id) {
                    let mut builder = BlockBuilder::new_from_sync_leap(
                        block_header,
                        maybe_sigs,
                        validator_weights,
                        peers,
                        should_fetch_execution_state,
                        self.max_simultaneous_peers,
                        self.config.peer_refresh_interval(),
                    );
                    apply_sigs(&mut builder, maybe_sigs);
                    if should_fetch_execution_state {
                        self.historical = Some(builder);
                    } else {
                        self.forward = Some(builder);
                    }
                } else {
                    warn!(
                        block_hash = %block_header.block_hash(),
                        "BlockSynchronizer: register_sync_leap unable to create block builder",
                    );
                }
            }
        }
    }

    /// Registers peers to a block builder by `BlockHash`.
    pub(crate) fn register_peers(&mut self, block_hash: BlockHash, peers: Vec<NodeId>) {
        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                builder.register_peers(peers);
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            }
        }
    }

    /* EVENT LOGIC */

    fn register_block_execution_not_enqueued(&mut self, block_hash: &BlockHash) {
        if let Some(builder) = &self.historical {
            if builder.block_hash() == *block_hash {
                error!(%block_hash, "historical block should not be enqueued for execution");
            }
        }

        match &mut self.forward {
            Some(builder) if builder.block_hash() == *block_hash => {
                builder.register_block_execution_not_enqueued();
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing forward block");
            }
        }
    }

    fn register_block_execution_enqueued(&mut self, block_hash: &BlockHash) {
        if let Some(builder) = &self.historical {
            if builder.block_hash() == *block_hash {
                error!(%block_hash, "historical block should not be enqueued for execution");
            }
        }

        match &mut self.forward {
            Some(builder) if builder.block_hash() == *block_hash => {
                builder.register_block_execution_enqueued();
                self.metrics
                    .forward_block_sync_duration
                    .observe(builder.sync_start_time().elapsed().as_secs_f64());
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing forward block");
            }
        }
    }

    fn register_marked_complete<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_hash: &BlockHash,
    ) -> Effects<Event>
    where
        REv: From<BlockSynchronizerAnnouncement> + From<BlockCompleteConfirmationRequest> + Send,
    {
        if let Some(builder) = &self.forward {
            if builder.block_hash() == *block_hash {
                error!(%block_hash, "forward block should not be marked complete in block synchronizer");
            }
        }

        let mut effects = Effects::new();
        match &mut self.historical {
            Some(builder) if builder.block_hash() == *block_hash => {
                builder.register_marked_complete();
                // other components need to know that we've added an historical block
                // that they may be interested in
                if let Some(block) = builder.maybe_block() {
                    effects.extend(effect_builder.announce_completed_block(block).ignore());
                }
                self.metrics
                    .historical_block_sync_duration
                    .observe(builder.sync_start_time().elapsed().as_secs_f64());
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing historical block");
            }
        }
        effects
    }

    fn dishonest_peers(&self) -> Vec<NodeId> {
        let mut ret = vec![];
        if let Some(builder) = &self.forward {
            ret.extend(builder.dishonest_peers());
        }
        if let Some(builder) = &self.historical {
            ret.extend(builder.dishonest_peers());
        }
        ret
    }

    fn flush_dishonest_peers(&mut self) {
        if let Some(builder) = &mut self.forward {
            builder.flush_dishonest_peers();
        }
        if let Some(builder) = &mut self.historical {
            builder.flush_dishonest_peers();
        }
    }

    fn need_next<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
    ) -> Effects<Event>
    where
        REv: ReactorEvent + From<FetcherRequest<Block>> + From<BlockCompleteConfirmationRequest>,
    {
        let need_next_interval = self.config.need_next_interval().into();
        let mut results = Effects::new();
        let max_simultaneous_peers = self.max_simultaneous_peers as usize;
        let mut builder_needs_next = |builder: &mut BlockBuilder| {
            if builder.in_flight_latch().is_some() || builder.is_finished() {
                return;
            }
            let action = builder.block_acquisition_action(rng);
            let peers = action.peers_to_ask();
            let need_next = action.need_next();
            info!("BlockSynchronizer: {}", need_next);
            match need_next {
                NeedNext::Nothing(_) => {
                    // currently idle or waiting, check back later
                    results.extend(
                        effect_builder
                            .set_timeout(need_next_interval)
                            .event(|_| Event::Request(BlockSynchronizerRequest::NeedNext)),
                    );
                }
                NeedNext::BlockHeader(block_hash) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockHeader>(block_hash, node_id, EmptyValidationMetadata)
                            .event(Event::BlockHeaderFetched)
                    }))
                }
                NeedNext::BlockBody(block_hash) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Block>(block_hash, node_id, EmptyValidationMetadata)
                            .event(Event::BlockFetched)
                    }))
                }
                NeedNext::FinalitySignatures(block_hash, era_id, validators) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        validators.iter().flat_map(move |public_key| {
                            let id = FinalitySignatureId {
                                block_hash,
                                era_id,
                                public_key: public_key.clone(),
                            };
                            effect_builder
                                .fetch::<FinalitySignature>(id, node_id, EmptyValidationMetadata)
                                .event(Event::FinalitySignatureFetched)
                        })
                    }))
                }
                NeedNext::GlobalState(block_hash, global_state_root_hash) => {
                    builder.set_in_flight_latch();
                    results.extend(
                        effect_builder
                            .sync_global_state(
                                block_hash,
                                global_state_root_hash,
                                peers.into_iter().collect(),
                            )
                            .event(move |result| Event::GlobalStateSynced { block_hash, result }),
                    );
                }
                NeedNext::ExecutionResultsChecksum(block_hash, global_state_root_hash) => {
                    builder.set_in_flight_latch();
                    results.extend(
                        effect_builder
                            .get_execution_results_checksum(global_state_root_hash)
                            .event(move |result| Event::GotExecutionResultsChecksum {
                                block_hash,
                                result,
                            }),
                    );
                }
                NeedNext::ExecutionResults(block_hash, id, checksum) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockExecutionResultsOrChunk>(id, node_id, checksum)
                            .event(move |result| Event::ExecutionResultsFetched {
                                block_hash,
                                result,
                            })
                    }))
                }
                NeedNext::ApprovalsHashes(block_hash, block) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<ApprovalsHashes>(block_hash, node_id, *block.clone())
                            .event(Event::ApprovalsHashesFetched)
                    }))
                }
                NeedNext::DeployByHash(block_hash, deploy_hash) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<LegacyDeploy>(deploy_hash, node_id, EmptyValidationMetadata)
                            .event(move |result| Event::DeployFetched {
                                block_hash,
                                result: Either::Left(result),
                            })
                    }))
                }
                NeedNext::DeployById(block_hash, deploy_id) => {
                    builder.set_in_flight_latch();
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Deploy>(deploy_id, node_id, EmptyValidationMetadata)
                            .event(move |result| Event::DeployFetched {
                                block_hash,
                                result: Either::Right(result),
                            })
                    }))
                }
                NeedNext::EnqueueForExecution(block_hash, _) => {
                    if false == builder.should_fetch_execution_state() {
                        builder.set_in_flight_latch();
                        results.extend(
                            effect_builder
                                .make_block_executable(block_hash)
                                .event(move |result| Event::MadeFinalizedBlock {
                                    block_hash,
                                    result,
                                }),
                        )
                    }
                }
                NeedNext::BlockMarkedComplete(block_hash, block_height) => {
                    // Only mark the block complete if we're syncing historical
                    // because we have global state and execution effects (if
                    // any).
                    if builder.should_fetch_execution_state() {
                        builder.set_in_flight_latch();
                        results.extend(
                            effect_builder
                                .mark_block_completed(block_height)
                                .event(move |_| Event::MarkBlockCompleted(block_hash)),
                        )
                    }
                }
                NeedNext::Peers(block_hash) => {
                    builder.set_in_flight_latch();
                    if builder.should_fetch_execution_state() {
                        // the accumulator may or may not have peers for an older block,
                        // so we're going to also get a random sampling from networking
                        results.extend(
                            effect_builder
                                .get_fully_connected_peers(max_simultaneous_peers)
                                .event(move |peers| Event::NetworkPeers(block_hash, peers)),
                        )
                    }
                    results.extend(
                        effect_builder
                            .get_block_accumulated_peers(block_hash)
                            .event(move |maybe_peers| {
                                Event::AccumulatedPeers(block_hash, maybe_peers)
                            }),
                    )
                }
                NeedNext::EraValidators(era_id) => {
                    warn!(
                        "BlockSynchronizer: does not have era_validators for era_id: {}",
                        era_id
                    );
                    results.extend(
                        effect_builder
                            .set_timeout(need_next_interval)
                            .event(|_| Event::Request(BlockSynchronizerRequest::NeedNext)),
                    )
                }
            }
        };

        if let Some(builder) = &mut self.forward {
            builder_needs_next(builder);
        }
        if let Some(builder) = &mut self.historical {
            builder_needs_next(builder);
        }
        results
    }

    fn register_disconnected_peer(&mut self, node_id: NodeId) {
        if let Some(builder) = &mut self.forward {
            builder.disqualify_peer(Some(node_id));
        }
        if let Some(builder) = &mut self.historical {
            builder.disqualify_peer(Some(node_id));
        }
    }

    fn block_header_fetched(
        &mut self,
        result: Result<FetchedData<BlockHeader>, FetcherError<BlockHeader>>,
    ) {
        let (block_hash, maybe_block_header, maybe_peer_id): (
            BlockHash,
            Option<Box<BlockHeader>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item.id(), Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item.id(), Some(item), None),
            Err(err) => {
                debug!(%err, "BlockSynchronizer: failed to fetch block header");
                if err.is_peer_fault() {
                    (*err.id(), None, Some(*err.peer()))
                } else {
                    (*err.id(), None, None)
                }
            }
        };

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                match maybe_block_header {
                    None => {
                        builder.demote_peer(maybe_peer_id);
                    }
                    Some(block_header) => {
                        if let Err(error) =
                            builder.register_block_header(*block_header, maybe_peer_id)
                        {
                            error!(%error, "BlockSynchronizer: failed to apply block header");
                        } else {
                            builder.register_era_validator_weights(&self.validator_matrix);
                        }
                    }
                }
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            }
        }
    }

    fn block_fetched(&mut self, result: Result<FetchedData<Block>, FetcherError<Block>>) {
        let (block_hash, maybe_block, maybe_peer_id): (
            BlockHash,
            Option<Box<Block>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => {
                debug!(
                    "BlockSynchronizer: fetched body {:?} from peer {}",
                    item.hash(),
                    peer
                );
                (*item.hash(), Some(item), Some(peer))
            }
            Ok(FetchedData::FromStorage { item }) => (*item.hash(), Some(item), None),
            Err(err) => {
                debug!(%err, "BlockSynchronizer: failed to fetch block");
                if err.is_peer_fault() {
                    (*err.id(), None, Some(*err.peer()))
                } else {
                    (*err.id(), None, None)
                }
            }
        };

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                match maybe_block {
                    None => {
                        builder.demote_peer(maybe_peer_id);
                    }
                    Some(block) => {
                        if let Err(error) = builder.register_block(&block, maybe_peer_id) {
                            error!(%error, "BlockSynchronizer: failed to apply block");
                        }
                    }
                }
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            }
        }
    }

    fn approvals_hashes_fetched(
        &mut self,
        result: Result<FetchedData<ApprovalsHashes>, FetcherError<ApprovalsHashes>>,
    ) {
        let (block_hash, maybe_approvals_hashes, maybe_peer_id): (
            BlockHash,
            Option<Box<ApprovalsHashes>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => {
                debug!(
                    "BlockSynchronizer: fetched approvals hashes {:?} from peer {}",
                    item.block_hash(),
                    peer
                );
                (*item.block_hash(), Some(item), Some(peer))
            }
            Ok(FetchedData::FromStorage { item }) => (*item.block_hash(), Some(item), None),
            Err(err) => {
                debug!(%err, "BlockSynchronizer: failed to fetch approvals hashes");
                if err.is_peer_fault() {
                    (*err.id(), None, Some(*err.peer()))
                } else {
                    (*err.id(), None, None)
                }
            }
        };

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                match maybe_approvals_hashes {
                    None => {
                        builder.demote_peer(maybe_peer_id);
                    }
                    Some(approvals_hashes) => {
                        if let Err(error) =
                            builder.register_approvals_hashes(&approvals_hashes, maybe_peer_id)
                        {
                            error!(%error, "BlockSynchronizer: failed to apply approvals hashes");
                        }
                    }
                }
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            }
        }
    }

    fn finality_signature_fetched(
        &mut self,
        result: Result<FetchedData<FinalitySignature>, FetcherError<FinalitySignature>>,
    ) {
        let (id, maybe_finality_signature, maybe_peer) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => {
                debug!(
                    "BlockSynchronizer: fetched finality signature {} from peer {}",
                    item, peer
                );
                (item.id(), Some(item), Some(peer))
            }
            Ok(FetchedData::FromStorage { item }) => (item.id(), Some(item), None),
            Err(err) => {
                debug!(%err, "BlockSynchronizer: failed to fetch finality signature");
                if err.is_peer_fault() {
                    (err.id().clone(), None, Some(*err.peer()))
                } else {
                    (err.id().clone(), None, None)
                }
            }
        };

        let block_hash = id.block_hash;

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                match maybe_finality_signature {
                    None => {
                        builder.demote_peer(maybe_peer);
                    }
                    Some(finality_signature) => {
                        if let Err(error) =
                            builder.register_finality_signature(*finality_signature, maybe_peer)
                        {
                            warn!(%error, "BlockSynchronizer: failed to apply finality signature");
                        }
                    }
                }
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            }
        }
    }

    fn global_state_synced(
        &mut self,
        block_hash: BlockHash,
        result: Result<Digest, GlobalStateSynchronizerError>,
    ) {
        let root_hash = match result {
            Ok(hash) => hash,
            Err(error) => {
                debug!(%error, "BlockSynchronizer: failed to sync global state");
                return;
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "BlockSynchronizer: not currently synchronising block");
            } else if let Err(error) = builder.register_global_state(root_hash) {
                error!(%block_hash, %error, "BlockSynchronizer: failed to apply global state");
            }
        }
    }

    fn got_execution_results_checksum(
        &mut self,
        block_hash: BlockHash,
        result: Result<Option<Digest>, engine_state::Error>,
    ) {
        let execution_results_checksum = match result {
            Ok(Some(digest)) => {
                debug!(
                    "BlockSynchronizer: got execution_results_checksum for {}",
                    block_hash
                );
                ExecutionResultsChecksum::Checkable(digest)
            }
            Err(engine_state::Error::MissingChecksumRegistry) => {
                // The registry will not exist for legacy blocks.
                ExecutionResultsChecksum::Uncheckable
            }
            Ok(None) => {
                warn!("BlockSynchronizer: the checksum registry should contain the execution results checksum");
                ExecutionResultsChecksum::Uncheckable
            }
            Err(error) => {
                error!(%error, "BlockSynchronizer: unexpected error getting checksum registry");
                ExecutionResultsChecksum::Uncheckable
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "BlockSynchronizer: not currently synchronising block");
            } else if let Err(error) =
                builder.register_execution_results_checksum(execution_results_checksum)
            {
                error!(%block_hash, %error, "BlockSynchronizer: failed to apply execution results checksum");
            }
        }
    }

    fn execution_results_fetched<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_hash: BlockHash,
        result: FetchResult<BlockExecutionResultsOrChunk>,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest> + Send,
    {
        let (maybe_value_or_chunk, maybe_peer_id) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => {
                debug!(
                    "BlockSynchronizer: fetched execution results {} from peer {}",
                    item.block_hash(),
                    peer
                );
                (Some(item), Some(peer))
            }
            Ok(FetchedData::FromStorage { item }) => (Some(item), None),
            Err(err) => {
                debug!(%err, "BlockSynchronizer: failed to fetch execution results or chunk");
                if err.is_peer_fault() {
                    (None, Some(*err.peer()))
                } else {
                    (None, None)
                }
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
                return Effects::new();
            }

            match maybe_value_or_chunk {
                None => {
                    builder.demote_peer(maybe_peer_id);
                }
                Some(value_or_chunk) => {
                    // due to reasons, the stitched back together execution effects need to be saved
                    // to disk here, when the last chunk is collected.
                    // we expect a response back, which will crank the block builder for this block
                    // to the next state.
                    match builder.register_fetched_execution_results(maybe_peer_id, *value_or_chunk)
                    {
                        Ok(Some(execution_results)) => {
                            return effect_builder
                                .put_execution_results_to_storage(block_hash, execution_results)
                                .event(move |()| Event::ExecutionResultsStored(block_hash));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            error!(%block_hash, %error, "BlockSynchronizer: failed to apply execution results or chunk");
                        }
                    }
                }
            }
        }
        Effects::new()
    }

    fn register_execution_results_stored(&mut self, block_hash: BlockHash) {
        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            } else if let Err(error) = builder.register_execution_results_stored_notification() {
                error!(%block_hash, %error, "BlockSynchronizer: failed to apply stored execution results");
            }
        }
    }

    fn deploy_fetched(&mut self, block_hash: BlockHash, fetched_deploy: FetchedData<Deploy>) {
        let (deploy, maybe_peer) = match fetched_deploy {
            FetchedData::FromPeer { item, peer } => (item, Some(peer)),
            FetchedData::FromStorage { item } => (item, None),
        };

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                if let Err(error) = builder.register_deploy(deploy.id(), maybe_peer) {
                    error!(%block_hash, %error, "BlockSynchronizer: failed to apply deploy");
                }
            }
            _ => {
                trace!(%block_hash, "BlockSynchronizer: not currently synchronizing block");
            }
        }
    }

    fn progress(&self, builder: &BlockBuilder) -> BlockSynchronizerProgress {
        if builder.is_finished() {
            match builder.block_height_and_era() {
                None => {
                    error!("BlockSynchronizer: finished builder should have block height and era")
                }
                Some((block_height, era_id)) => {
                    return BlockSynchronizerProgress::Synced(
                        builder.block_hash(),
                        block_height,
                        era_id,
                    )
                }
            }
        }
        BlockSynchronizerProgress::Syncing(
            builder.block_hash(),
            builder.block_height(),
            builder.last_progress_time().max(
                self.global_sync
                    .last_progress()
                    .unwrap_or_else(Timestamp::zero),
            ),
        )
    }

    fn status(&self) -> BlockSynchronizerStatus {
        BlockSynchronizerStatus::new(
            self.historical.as_ref().map(|builder| BlockSyncStatus {
                block_hash: builder.block_hash(),
                block_height: builder.block_height(),
                acquisition_state: builder.block_acquisition_state().to_string(),
            }),
            self.forward.as_ref().map(|builder| BlockSyncStatus {
                block_hash: builder.block_hash(),
                block_height: builder.block_height(),
                acquisition_state: builder.block_acquisition_state().to_string(),
            }),
        )
    }
}

impl<REv> InitializedComponent<REv> for BlockSynchronizer
where
    REv: ReactorEvent + From<FetcherRequest<Block>>,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }

    fn name(&self) -> &str {
        "block_synchronizer"
    }
}

impl<REv: ReactorEvent> Component<REv> for BlockSynchronizer {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.status {
            ComponentStatus::Fatal(msg) => {
                error!(
                    msg,
                    "BlockSynchronizer: should not handle this event when this component has fatal error"
                );
                return Effects::new();
            }
            ComponentStatus::Uninitialized => {
                return if matches!(event, Event::Initialize) {
                    self.status = ComponentStatus::Initialized;
                    // start dishonest peer management on initialization
                    effect_builder
                        .set_timeout(self.config.disconnect_dishonest_peers_interval().into())
                        .event(move |_| Event::Request(BlockSynchronizerRequest::DishonestPeers))
                } else {
                    warn!("BlockSynchronizer: should not handle this event when component is uninitialized");
                    Effects::new()
                };
            }
            ComponentStatus::Initialized => (),
        }

        return match event {
            Event::Initialize => Effects::new(), //noop
            Event::Request(request) => match request {
                // the rpc and rest servers include block sync data on their status responses
                BlockSynchronizerRequest::Status { responder } => {
                    responder.respond(self.status()).ignore()
                }
                // prompts for what data (if any) is needed next to acquire block(s) being sync'd
                BlockSynchronizerRequest::NeedNext => self.need_next(effect_builder, rng),
                // this component is periodically asked for any peers that have provided false
                // data (if any) which are then disconnected from
                BlockSynchronizerRequest::DishonestPeers => {
                    let mut effects: Effects<Self::Event> = self
                        .dishonest_peers()
                        .into_iter()
                        .flat_map(|node_id| {
                            effect_builder
                                .announce_block_peer_with_justification(
                                    node_id,
                                    BlocklistJustification::DishonestPeer,
                                )
                                .ignore()
                        })
                        .collect();
                    self.flush_dishonest_peers();
                    effects.extend(
                        effect_builder
                            .set_timeout(self.config.disconnect_dishonest_peers_interval().into())
                            .event(move |_| {
                                Event::Request(BlockSynchronizerRequest::DishonestPeers)
                            }),
                    );
                    effects
                }
            },
            // tunnel event to global state synchronizer
            // global_state_sync is a black box; we do not hook need next here
            // global_state_sync signals the historical sync builder at the end of its process,
            // and need next is then re-hooked to get the rest of the block
            Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                Event::GlobalStateSynchronizer,
                self.global_sync.handle_event(effect_builder, rng, event),
            ),
            // when a peer is disconnected from for any reason, disqualify peer
            Event::DisconnectFromPeer(node_id) => {
                self.register_disconnected_peer(node_id);
                Effects::new()
            }
            // each of the following trigger the next need next
            Event::ValidatorMatrixUpdated => {
                let mut effects = self.handle_validators(effect_builder);
                effects.extend(self.need_next(effect_builder, rng));
                effects
            }
            // for both historical and forward sync, the block header has been fetched
            Event::BlockHeaderFetched(result) => {
                self.block_header_fetched(result);
                self.need_next(effect_builder, rng)
            }
            // for both historical and forward sync, the block body has been fetched
            Event::BlockFetched(result) => {
                self.block_fetched(result);
                self.need_next(effect_builder, rng)
            }
            // for both historical and forward sync, a finality signature has been fetched
            Event::FinalitySignatureFetched(result) => {
                self.finality_signature_fetched(result);
                self.need_next(effect_builder, rng)
            }
            // for both historical and forward sync, post-1.4 blocks track approvals hashes
            // for the deploys they contain
            Event::ApprovalsHashesFetched(result) => {
                self.approvals_hashes_fetched(result);
                self.need_next(effect_builder, rng)
            }
            // we use the existence of n execution results checksum as an expedient way to
            // determine if a block is post-1.4
            Event::GotExecutionResultsChecksum { block_hash, result } => {
                self.got_execution_results_checksum(block_hash, result);
                self.need_next(effect_builder, rng)
            }
            // historical sync needs to know that global state has been sync'd
            Event::GlobalStateSynced { block_hash, result } => {
                self.global_state_synced(block_hash, result);
                self.need_next(effect_builder, rng)
            }
            // historical sync needs to know that execution results have been fetched
            Event::ExecutionResultsFetched { block_hash, result } => {
                let mut effects =
                    self.execution_results_fetched(effect_builder, block_hash, result);
                effects.extend(self.need_next(effect_builder, rng));
                effects
            }
            // historical sync needs to know that execution effects have been stored
            Event::ExecutionResultsStored(block_hash) => {
                self.register_execution_results_stored(block_hash);
                self.need_next(effect_builder, rng)
            }
            // for pre-1.5 blocks we use the legacy deploy fetcher, otherwise we use the deploy
            // fetcher but the results of both are forwarded to this handler
            Event::DeployFetched { block_hash, result } => {
                match result {
                    Either::Left(Ok(fetched_legacy_deploy)) => {
                        let deploy_id = fetched_legacy_deploy.id();
                        debug!(%block_hash, ?deploy_id, "BlockSynchronizer: fetched legacy deploy");
                        self.deploy_fetched(block_hash, fetched_legacy_deploy.convert())
                    }
                    Either::Right(Ok(fetched_deploy)) => {
                        let deploy_id = fetched_deploy.id();
                        debug!(%block_hash, ?deploy_id, "BlockSynchronizer: fetched deploy");
                        self.deploy_fetched(block_hash, fetched_deploy)
                    }
                    Either::Left(Err(error)) => {
                        debug!(%error, "BlockSynchronizer: failed to fetch legacy deploy");
                    }
                    Either::Right(Err(error)) => {
                        debug!(%error, "BlockSynchronizer: failed to fetch deploy");
                    }
                };
                self.need_next(effect_builder, rng)
            }
            // fresh peers to apply (random sample from network)
            Event::NetworkPeers(block_hash, peers) => {
                debug!(%block_hash, "BlockSynchronizer: got {} peers from network", peers.len());
                self.register_peers(block_hash, peers);
                self.need_next(effect_builder, rng)
            }
            // fresh peers to apply (qualified peers from accumulator)
            Event::AccumulatedPeers(block_hash, Some(peers)) => {
                debug!(%block_hash, "BlockSynchronizer: got {} peers from accumulator", peers.len());
                self.register_peers(block_hash, peers);
                self.need_next(effect_builder, rng)
            }
            // no more peers available, what do we need next?
            Event::AccumulatedPeers(block_hash, None) => {
                debug!(%block_hash, "BlockSynchronizer: got 0 peers from accumulator");
                self.need_next(effect_builder, rng)
            }

            // do not hook need next for the following events;
            Event::MadeFinalizedBlock { block_hash, result } => {
                // when syncing a forward block the node does not acquire
                // global state and execution results from peers; instead
                // the node attempts to execute the block to produce the
                // global state and execution results and check the results
                // first, the block it must be turned into a finalized block
                // and then enqueued for execution.
                let mut effects = Effects::new();
                match result {
                    Some((finalized_block, deploys)) => {
                        effects.extend(
                            effect_builder
                                .enqueue_block_for_execution(finalized_block, deploys)
                                .event(move |_| Event::MarkBlockExecutionEnqueued(block_hash)),
                        );
                    }
                    None => self.register_block_execution_not_enqueued(&block_hash),
                }
                effects
            }
            Event::MarkBlockExecutionEnqueued(block_hash) => {
                // when syncing a forward block the synchronizer considers it
                // finished after it has been successfully enqueued for execution
                self.register_block_execution_enqueued(&block_hash);
                Effects::new()
            }
            Event::MarkBlockCompleted(block_hash) => {
                // when syncing an historical block, the synchronizer considers it
                // finished after receiving confirmation that the complete block
                // has been stored.
                self.register_marked_complete(effect_builder, &block_hash)
            }
        };
    }
}

impl<REv: ReactorEvent> ValidatorBoundComponent<REv> for BlockSynchronizer {
    fn handle_validators(&mut self, _: EffectBuilder<REv>) -> Effects<Self::Event> {
        if let Some(block_builder) = &mut self.forward {
            block_builder.register_era_validator_weights(&self.validator_matrix);
        }
        if let Some(block_builder) = &mut self.historical {
            block_builder.register_era_validator_weights(&self.validator_matrix);
        }
        Effects::new()
    }
}
