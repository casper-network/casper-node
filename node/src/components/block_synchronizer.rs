mod block_acquisition;
mod block_builder;
mod config;
mod deploy_acquisition;
mod event;
mod execution_results_acquisition;
mod global_state_synchronizer;
mod need_next;
mod peer_list;
mod signature_acquisition;
mod trie_accumulator;

use std::time::Duration;

use datasize::DataSize;
use either::Either;
use tracing::{debug, error, warn};

use casper_execution_engine::core::engine_state;
use casper_hashing::Digest;
use casper_types::{TimeDiff, Timestamp};

use crate::{
    components::{
        fetcher::{Error, FetchResult, FetchedData},
        Component, ComponentStatus, InitializedComponent, ValidatorBoundComponent,
    },
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{
            BlockAccumulatorRequest, BlockCompleteConfirmationRequest, BlockSynchronizerRequest,
            ContractRuntimeRequest, FetcherRequest, StorageRequest, SyncGlobalStateRequest,
            TrieAccumulatorRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor::{self},
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHash, BlockHeader,
        BlockSignatures, Deploy, EmptyValidationMetadata, FinalitySignature, FinalitySignatureId,
        Item, LegacyDeploy, NodeId, SyncLeap, TrieOrChunk, ValidatorMatrix,
    },
    NodeRng,
};
use block_builder::BlockBuilder;
pub(crate) use config::Config;
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

pub(crate) trait ReactorEvent:
    From<FetcherRequest<ApprovalsHashes>>
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
    + Send
    + 'static
{
}

impl<REv> ReactorEvent for REv where
    REv: From<FetcherRequest<ApprovalsHashes>>
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
        + Send
        + 'static
{
}

// todo!() - remove Debug derive.
#[derive(Debug)]
pub(crate) enum BlockSynchronizerProgress {
    Idle,
    Syncing(BlockHash, Option<u64>, Timestamp),
    Synced(BlockHash, u64),
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockSynchronizer {
    status: ComponentStatus,
    validator_matrix: ValidatorMatrix,
    timeout: TimeDiff,
    peer_refresh_interval: TimeDiff,
    need_next_interval: TimeDiff,
    // we pause block_syncing if a node is actively validating
    paused: bool,

    // execute forward block (do not get global state or execution effects)
    forward: Option<BlockBuilder>,
    // either sync-to-genesis or sync-leaped block (get global state and execution effects)
    historical: Option<BlockBuilder>,
    // deals with global state acquisition for historical blocks
    global_sync: GlobalStateSynchronizer,
}

impl BlockSynchronizer {
    pub(crate) fn new(config: Config, validator_matrix: ValidatorMatrix) -> Self {
        BlockSynchronizer {
            status: ComponentStatus::Uninitialized,
            validator_matrix,
            timeout: config.timeout(),
            peer_refresh_interval: config.peer_refresh_interval(),
            need_next_interval: config.need_next_interval(),
            paused: false,
            forward: None,
            historical: None,
            global_sync: GlobalStateSynchronizer::new(config.max_parallel_trie_fetches() as usize),
        }
    }

    /// Returns the progress being made on the historical syncing.
    pub(crate) fn catch_up_progress(&mut self) -> BlockSynchronizerProgress {
        match &self.historical {
            None => BlockSynchronizerProgress::Idle,
            Some(builder) => {
                if builder.is_finished() {
                    match builder.block_height() {
                        None => error!("finished builder should have block height"),
                        Some(block_height) => {
                            return BlockSynchronizerProgress::Synced(
                                builder.block_hash(),
                                block_height,
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
        }
    }

    /// Returns the progress being made on the historical syncing.
    pub(crate) fn keep_up_progress(&mut self) -> BlockSynchronizerProgress {
        match &self.forward {
            None => BlockSynchronizerProgress::Idle,
            Some(builder) => {
                if builder.is_finished() {
                    match builder.block_height() {
                        None => error!("finished builder should have block height"),
                        Some(block_height) => {
                            return BlockSynchronizerProgress::Synced(
                                builder.block_hash(),
                                block_height,
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
        }
    }

    /// Pauses block synchronization.
    pub(crate) fn pause(&mut self) {
        self.paused = true;
    }

    /// Resumes block synchronization.
    pub(crate) fn resume(&mut self) {
        self.paused = false;
    }

    /// Registers a block for synchronization.
    pub(crate) fn register_block_by_hash(
        &mut self,
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        requires_strict_finality: bool,
        max_simultaneous_peers: u32,
    ) {
        if should_fetch_execution_state {
            if let Some(block_builder) = &self.historical {
                if block_builder.block_hash() == block_hash {
                    return;
                }
            }
            let builder = BlockBuilder::new(
                block_hash,
                should_fetch_execution_state,
                requires_strict_finality,
                max_simultaneous_peers,
                self.peer_refresh_interval,
            );
            self.historical.replace(builder);
        } else {
            if let Some(block_builder) = &self.forward {
                if block_builder.block_hash() == block_hash {
                    return;
                }
            }
            let builder = BlockBuilder::new(
                block_hash,
                should_fetch_execution_state,
                requires_strict_finality,
                max_simultaneous_peers,
                self.peer_refresh_interval,
            );
            self.forward.replace(builder);
        }
    }

    /// Registers a sync leap result, if able.
    pub(crate) fn register_sync_leap(
        &mut self,
        sync_leap: &SyncLeap,
        peers: Vec<NodeId>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) {
        fn apply_sigs(builder: &mut BlockBuilder, maybe_sigs: Option<&BlockSignatures>) {
            if let Some(signatures) = maybe_sigs {
                for finality_signature in signatures.finality_signatures() {
                    if let Err(error) =
                        builder.register_finality_signature(finality_signature, None)
                    {
                        debug!(%error, "failed to register finality signature");
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
            }
            _ => {
                let era_id = block_header.era_id();
                if let Some(validator_weights) = self.validator_matrix.validator_weights(era_id) {
                    let mut builder = BlockBuilder::new_from_sync_leap(
                        sync_leap,
                        validator_weights,
                        peers,
                        should_fetch_execution_state,
                        max_simultaneous_peers,
                        self.peer_refresh_interval,
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
                        "unable to create block builder",
                    );
                }
            }
        }
    }

    /* EVENT LOGIC */

    fn register_marked_complete(&mut self, block_hash: &BlockHash) {
        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == *block_hash => {
                builder.register_marked_complete();
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }
    }

    fn register_peers(&mut self, block_hash: BlockHash, peers: Vec<NodeId>) -> Effects<Event> {
        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                builder.register_peers(peers);
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }
        Effects::new()
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

    fn hook_need_next<REv: Send>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut effects: Effects<Event>,
    ) -> Effects<Event> {
        effects.extend(
            effect_builder
                .set_timeout(self.need_next_interval.into())
                .event(|_| Event::Request(BlockSynchronizerRequest::NeedNext)),
        );
        effects
    }

    fn need_next<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
    ) -> Effects<Event>
    where
        REv: ReactorEvent + From<FetcherRequest<Block>> + From<BlockCompleteConfirmationRequest>,
    {
        let need_next_interval = self.need_next_interval.into();
        let mut results = Effects::new();
        let mut builder_needs_next = |builder: &mut BlockBuilder| {
            let action = builder.block_acquisition_action(rng);
            let peers = action.peers_to_ask(); // pass this to any fetcher
            let need_next = action.need_next();
            debug!("BlockSynchronizer: {:?}", need_next);
            if !matches!(need_next, NeedNext::Nothing) {
                builder.set_in_flight_latch(true);
            }
            match need_next {
                NeedNext::Nothing => {}
                NeedNext::BlockHeader(block_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockHeader>(block_hash, node_id, EmptyValidationMetadata)
                            .event(Event::BlockHeaderFetched)
                    }))
                }
                NeedNext::BlockBody(block_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Block>(block_hash, node_id, EmptyValidationMetadata)
                            .event(Event::BlockFetched)
                    }))
                }
                NeedNext::FinalitySignatures(block_hash, era_id, validators) => {
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
                NeedNext::GlobalState(block_hash, global_state_root_hash) => results.extend(
                    effect_builder
                        .sync_global_state(
                            block_hash,
                            global_state_root_hash,
                            peers.into_iter().collect(),
                        )
                        .event(move |result| Event::GlobalStateSynced { block_hash, result }),
                ),
                NeedNext::ApprovalsHashes(block_hash, block) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<ApprovalsHashes>(block_hash, node_id, *block.clone())
                            .event(Event::ApprovalsHashesFetched)
                    }))
                }
                NeedNext::ExecutionResultsRootHash(block_hash, global_state_root_hash) => {
                    results.extend(
                        effect_builder
                            .get_execution_results_root_hash(global_state_root_hash)
                            .event(move |result| Event::GotExecutionResultsRootHash {
                                block_hash,
                                result,
                            }),
                    );
                }
                NeedNext::DeployByHash(block_hash, deploy_hash) => {
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
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Deploy>(deploy_id, node_id, EmptyValidationMetadata)
                            .event(move |result| Event::DeployFetched {
                                block_hash,
                                result: Either::Right(result),
                            })
                    }))
                }
                NeedNext::ExecutionResults(block_hash, id, checksum) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockExecutionResultsOrChunk>(id, node_id, checksum)
                            .event(move |result| Event::ExecutionResultsFetched {
                                block_hash,
                                result,
                            })
                    }))
                }
                NeedNext::MarkComplete(block_hash, block_height) => results.extend(
                    effect_builder
                        .mark_block_completed(block_height)
                        .event(move |_| Event::MarkedComplete(block_hash)),
                ),
                NeedNext::EraValidators(era_id) => {
                    warn!(
                        "block_synchronizer does not have era_validators for era_id: {}",
                        era_id
                    );
                    results.extend(
                        effect_builder
                            .set_timeout(need_next_interval)
                            .event(|_| Event::Request(BlockSynchronizerRequest::NeedNext)),
                    )
                }
                NeedNext::Peers(block_hash) => results.extend(
                    effect_builder
                        .get_block_accumulated_peers(block_hash)
                        .event(move |maybe_peers| Event::AccumulatedPeers(block_hash, maybe_peers)),
                ),
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

    fn register_disconnected_peer(&mut self, node_id: NodeId) -> Effects<Event> {
        if let Some(builder) = &mut self.forward {
            builder.disqualify_peer(Some(node_id));
        }
        if let Some(builder) = &mut self.historical {
            builder.disqualify_peer(Some(node_id));
        }
        Effects::new()
    }

    fn block_header_fetched(
        &mut self,
        result: Result<FetchedData<BlockHeader>, Error<BlockHeader>>,
    ) -> Effects<Event> {
        let (block_hash, maybe_block_header, maybe_peer_id): (
            BlockHash,
            Option<Box<BlockHeader>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item.id(), Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item.id(), Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch block header");
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
                            error!(%error, "failed to apply block header");
                        } else {
                            builder.register_era_validator_weights(&self.validator_matrix);
                        }
                    }
                }
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }
        Effects::new()
    }

    fn block_fetched(
        &mut self,
        result: Result<FetchedData<Block>, Error<Block>>,
    ) -> Effects<Event> {
        let (block_hash, maybe_block, maybe_peer_id): (
            BlockHash,
            Option<Box<Block>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => {
                debug!(
                    "BlockSynchronizer: fetched {} from peer {:?}",
                    item.hash(),
                    peer
                );
                (*item.hash(), Some(item), Some(peer))
            }
            Ok(FetchedData::FromStorage { item }) => (*item.hash(), Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch block");
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
                            error!(%error, "failed to apply block");
                        }
                    }
                }
            }
            _ => {
                warn!(%block_hash, "not currently synchronizing block");
            }
        }
        Effects::new()
    }

    fn approvals_hashes_fetched(
        &mut self,
        result: Result<FetchedData<ApprovalsHashes>, Error<ApprovalsHashes>>,
    ) -> Effects<Event> {
        let (block_hash, maybe_approvals_hashes, maybe_peer_id): (
            BlockHash,
            Option<Box<ApprovalsHashes>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => {
                (*item.block_hash(), Some(item), Some(peer))
            }
            Ok(FetchedData::FromStorage { item }) => (*item.block_hash(), Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch approvals hashes");
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
                            error!(%error, "failed to apply approvals hashes");
                        }
                    }
                }
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }
        Effects::new()
    }

    fn finality_signature_fetched(
        &mut self,
        result: Result<FetchedData<FinalitySignature>, Error<FinalitySignature>>,
    ) -> Effects<Event> {
        let (id, maybe_finality_signature, maybe_peer) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item.id(), Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item.id(), Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch finality signature");
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
                            error!(%error, "failed to apply finality signature");
                        }
                    }
                }
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }

        Effects::new()
    }

    fn global_state_synced(
        &mut self,
        block_hash: BlockHash,
        result: Result<Digest, GlobalStateSynchronizerError>,
    ) -> Effects<Event> {
        let root_hash = match result {
            Ok(hash) => hash,
            Err(error) => {
                debug!(%error, "failed to sync global state");
                return Effects::new();
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "not currently synchronising block");
            } else if let Err(error) = builder.register_global_state(root_hash) {
                error!(%block_hash, %error, "failed to apply global state");
            }
        }
        Effects::new()
    }

    fn got_execution_results_root_hash(
        &mut self,
        block_hash: BlockHash,
        result: Result<Option<Digest>, engine_state::Error>,
    ) -> Effects<Event> {
        let execution_results_root_hash = match result {
            Ok(Some(digest)) => {
                debug!(
                    "BlockSynchronizer: got execution_results_root_hash for {:?}",
                    block_hash
                );
                ExecutionResultsChecksum::Checkable(digest)
            }
            Ok(None) => {
                warn!("the checksum registry should contain the execution results checksum");
                ExecutionResultsChecksum::Uncheckable
            }
            Err(engine_state::Error::MissingChecksumRegistry) => {
                // The registry will not exist for legacy blocks.
                ExecutionResultsChecksum::Uncheckable
            }
            Err(error) => {
                error!(%error, "unexpected error getting checksum registry");
                ExecutionResultsChecksum::Uncheckable
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "not currently synchronising block");
            } else if let Err(error) =
                builder.register_execution_results_root_hash(execution_results_root_hash)
            {
                error!(%block_hash, %error, "failed to apply execution results root hash");
            }
        }
        Effects::new()
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
            Ok(FetchedData::FromPeer { item, peer }) => (Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch execution results or chunk");
                if err.is_peer_fault() {
                    (None, Some(*err.peer()))
                } else {
                    (None, None)
                }
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "not currently synchronizing block");
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
                            error!(%block_hash, %error, "failed to apply execution results or chunk");
                        }
                    }
                }
            }
        }
        Effects::new()
    }

    fn register_execution_results_stored(&mut self, block_hash: BlockHash) -> Effects<Event> {
        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "not currently synchronizing block");
            } else if let Err(error) = builder.register_execution_results_stored_notification() {
                error!(%block_hash, %error, "failed to apply stored execution results");
            }
        }
        Effects::new()
    }

    fn deploy_fetched(
        &mut self,
        block_hash: BlockHash,
        fetched_deploy: FetchedData<Deploy>,
    ) -> Effects<Event> {
        let (deploy, maybe_peer) = match fetched_deploy {
            FetchedData::FromPeer { item, peer } => (item, Some(peer)),
            FetchedData::FromStorage { item } => (item, None),
        };

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                if let Err(error) = builder.register_deploy(deploy.id(), maybe_peer) {
                    error!(%block_hash, %error, "failed to apply deploy");
                }
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }

        Effects::new()
    }

    fn register_executed_block_notification(&mut self, block_hash: BlockHash, height: u64) {
        // if the block being synchronized for execution has already executed, drop it.
        let finished_with_forward = if let Some(builder) = self.forward.as_ref() {
            builder
                .block_height()
                .map_or(false, |forward_height| forward_height <= height)
                || builder.block_hash() == block_hash
        } else {
            false
        };

        if finished_with_forward {
            let builder = self
                .forward
                .take()
                .expect("must be Some due to check above");
            let forward_hash = builder.block_hash();
            self.global_sync
                .cancel_request(forward_hash, global_state_synchronizer::Error::Cancelled);
        }
    }
}

impl<REv> InitializedComponent<REv> for BlockSynchronizer
where
    REv: ReactorEvent + From<FetcherRequest<Block>>,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
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
        if self.paused {
            warn!(%event, "block synchronizer not currently enabled - ignoring event");
            return Effects::new();
        }

        // MISSING EVENT: ANNOUNCEMENT OF BAD PEERS
        match (&self.status, event) {
            (ComponentStatus::Fatal(msg), _) => {
                error!(
                    msg,
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            (ComponentStatus::Uninitialized, Event::Initialize) => {
                self.status = ComponentStatus::Initialized;
                // start dishonest peer management on initialization
                effect_builder
                    .set_timeout(Duration::from_secs(10))
                    .event(move |_| Event::Request(BlockSynchronizerRequest::DishonestPeers))
            }
            (ComponentStatus::Uninitialized, _) => {
                warn!("should not handle this event when component is uninitialized");
                Effects::new()
            }
            (ComponentStatus::Initialized, event) => {
                match event {
                    Event::Initialize => {
                        // noop
                        Effects::new()
                    }
                    Event::Request(BlockSynchronizerRequest::NeedNext) => {
                        self.need_next(effect_builder, rng)
                    }
                    Event::Request(BlockSynchronizerRequest::BlockExecuted {
                        block_hash,
                        height,
                    }) => {
                        self.register_executed_block_notification(block_hash, height);
                        Effects::new()
                    }
                    Event::Request(BlockSynchronizerRequest::DishonestPeers) => {
                        let mut effects: Effects<Self::Event> = self
                            .dishonest_peers()
                            .into_iter()
                            .flat_map(|node_id| {
                                effect_builder
                                    .announce_disconnect_from_peer(node_id)
                                    .ignore()
                            })
                            .collect();
                        self.flush_dishonest_peers();
                        effects.extend(effect_builder.set_timeout(Duration::from_secs(10)).event(
                            move |_| Event::Request(BlockSynchronizerRequest::DishonestPeers),
                        ));
                        effects
                    }

                    // tunnel event to global state synchronizer
                    Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                        Event::GlobalStateSynchronizer,
                        self.global_sync.handle_event(effect_builder, rng, event),
                    ),

                    // when a peer is disconnected from for any reason, disqualify peer
                    Event::DisconnectFromPeer(node_id) => self.register_disconnected_peer(node_id),

                    // triggered indirectly via need next effects, and they perpetuate
                    // another need next to keep it going
                    Event::ValidatorMatrixUpdated => {
                        self.handle_validators(effect_builder);
                        self.hook_need_next(effect_builder, Effects::new())
                    }
                    Event::BlockHeaderFetched(result) => {
                        let effects = self.block_header_fetched(result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::BlockFetched(result) => {
                        let effects = self.block_fetched(result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::FinalitySignatureFetched(result) => {
                        let effects = self.finality_signature_fetched(result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::ApprovalsHashesFetched(result) => {
                        let effects = self.approvals_hashes_fetched(result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::GotExecutionResultsRootHash { block_hash, result } => {
                        let effects = self.got_execution_results_root_hash(block_hash, result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::GlobalStateSynced { block_hash, result } => {
                        let effects = self.global_state_synced(block_hash, result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::ExecutionResultsFetched { block_hash, result } => {
                        let effects =
                            self.execution_results_fetched(effect_builder, block_hash, result);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::ExecutionResultsStored(block_hash) => {
                        let effects = self.register_execution_results_stored(block_hash);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::DeployFetched { block_hash, result } => {
                        let effects = match result {
                            Either::Left(Ok(fetched_legacy_deploy)) => {
                                self.deploy_fetched(block_hash, fetched_legacy_deploy.convert())
                            }
                            Either::Left(Err(error)) => {
                                debug!(%error, "failed to fetch legacy deploy");
                                Effects::new()
                            }
                            Either::Right(Ok(fetched_deploy)) => {
                                self.deploy_fetched(block_hash, fetched_deploy)
                            }
                            Either::Right(Err(error)) => {
                                debug!(%error, "failed to fetch deploy");
                                Effects::new()
                            }
                        };
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::MarkedComplete(block_hash) => {
                        self.register_marked_complete(&block_hash);
                        Effects::new()
                        // self.hook_need_next(effect_builder, Effects::new())
                    }

                    Event::AccumulatedPeers(block_hash, Some(peers)) => {
                        let effects = self.register_peers(block_hash, peers);
                        self.hook_need_next(effect_builder, effects)
                    }
                    Event::AccumulatedPeers(_, None) => {
                        self.hook_need_next(effect_builder, Effects::new())
                    }
                }
            }
        }
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
