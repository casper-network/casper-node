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

use std::{
    collections::{BTreeMap, HashMap},
    iter,
    time::Duration,
};

use datasize::DataSize;
use either::Either;
use num_rational::Ratio;
use tracing::{debug, error, warn};

use casper_execution_engine::core::engine_state;
use casper_hashing::Digest;
use casper_types::{EraId, PublicKey, TimeDiff, Timestamp, U512};

use crate::{
    components::{
        fetcher,
        fetcher::{Error, FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{
            BlockSynchronizerRequest, BlocksAccumulatorRequest, ContractRuntimeRequest,
            FetcherRequest, SyncGlobalStateRequest, TrieAccumulatorRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    reactor,
    storage::StorageRequest,
    types::{
        BlockExecutionResultsOrChunk, BlockHash, BlockHeader, BlockHeaderWithMetadata,
        BlockSignatures, Deploy, DeployHash, ExecutedBlock, FinalitySignature, FinalitySignatureId,
        Item, LegacyDeploy, NodeId, SyncLeap, TrieOrChunk, ValidatorMatrix,
    },
    NodeRng,
};
use block_builder::BlockBuilder;
pub(crate) use config::Config;
pub(crate) use event::Event;
use execution_results_acquisition::{ExecutionResultsAcquisition, ExecutionResultsChecksum};
use global_state_synchronizer::GlobalStateSynchronizer;
pub(crate) use global_state_synchronizer::{
    Error as GlobalStateSynchronizerError, Event as GlobalStateSynchronizerEvent,
};
pub(crate) use need_next::NeedNext;
use trie_accumulator::TrieAccumulator;
pub(crate) use trie_accumulator::{Error as TrieAccumulatorError, Event as TrieAccumulatorEvent};

pub(crate) trait ReactorEvent:
    From<FetcherRequest<ExecutedBlock>>
    + From<FetcherRequest<BlockHeader>>
    + From<FetcherRequest<LegacyDeploy>>
    + From<FetcherRequest<Deploy>>
    + From<FetcherRequest<FinalitySignature>>
    + From<FetcherRequest<TrieOrChunk>>
    + From<FetcherRequest<BlockExecutionResultsOrChunk>>
    + From<BlocksAccumulatorRequest>
    + From<PeerBehaviorAnnouncement>
    + From<StorageRequest>
    + From<TrieAccumulatorRequest>
    + From<ContractRuntimeRequest>
    + From<SyncGlobalStateRequest>
    + Send
    + 'static
{
}

impl<REv> ReactorEvent for REv where
    REv: From<FetcherRequest<ExecutedBlock>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<LegacyDeploy>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalitySignature>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<FetcherRequest<BlockExecutionResultsOrChunk>>
        + From<BlocksAccumulatorRequest>
        + From<PeerBehaviorAnnouncement>
        + From<StorageRequest>
        + From<TrieAccumulatorRequest>
        + From<ContractRuntimeRequest>
        + From<SyncGlobalStateRequest>
        + Send
        + 'static
{
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockSynchronizer {
    timeout: TimeDiff,
    forward: Option<BlockBuilder>,
    historical: Option<BlockBuilder>,
    validator_matrix: ValidatorMatrix,
    global_sync: GlobalStateSynchronizer,
    disabled: bool,
}

impl BlockSynchronizer {
    pub(crate) fn new(config: Config) -> Self {
        BlockSynchronizer {
            timeout: config.timeout(),
            forward: None,
            historical: None,
            validator_matrix: Default::default(),
            global_sync: GlobalStateSynchronizer::new(config.max_parallel_trie_fetches() as usize),
            disabled: false,
        }
    }

    // CALLED FROM REACTOR
    pub(crate) fn turn_on(&mut self) {
        self.disabled = false;
    }

    // CALLED FROM REACTOR
    pub(crate) fn turn_off(&mut self) {
        self.disabled = true;
    }

    // CALLED FROM REACTOR
    pub(crate) fn maybe_executable_block_hash(&self) -> Option<BlockHash> {
        if let Some(fwd) = &self.forward {
            return Some(fwd.block_hash());
        }
        None
    }

    // CALLED FROM REACTOR
    pub(crate) fn register_block_by_hash(
        &mut self,
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        requires_strict_finality: bool,
        max_simultaneous_peers: u32,
    ) {
        let builder = BlockBuilder::new(
            block_hash,
            should_fetch_execution_state,
            requires_strict_finality,
            max_simultaneous_peers,
        );
        if should_fetch_execution_state {
            if let Some(historical) = self.historical.replace(builder) {
                debug!(
                    block_hash = %historical.block_hash(),
                    "replacing current sync of historical complete block"
                );
            }
        } else if let Some(forward) = self.forward.replace(builder) {
            debug!(
                block_hash = %forward.block_hash(),
                "replacing current sync of forward complete block"
            );
        }
    }

    // CALLED FROM REACTOR
    pub(crate) fn last_progress(&self) -> Option<Timestamp> {
        self.forward
            .as_ref()
            .and_then(BlockBuilder::last_progress_time)
            .into_iter()
            .chain(
                self.historical
                    .as_ref()
                    .and_then(BlockBuilder::last_progress_time),
            )
            .max()
    }

    // CALLED FROM REACTOR
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

        let _ = sync_leap.apply_validator_weights(&mut self.validator_matrix);
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

    // EVENTED
    fn register_peers(&mut self, block_hash: BlockHash, peers: Vec<NodeId>) -> Effects<Event> {
        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                for peer in peers {
                    builder.register_peer(peer);
                }
            }
            _ => {
                debug!(%block_hash, "not currently synchronizing block");
            }
        }
        Effects::new()
    }

    // EVENTED
    fn register_needed_era_validators(
        &mut self,
        era_id: EraId,
        validators: BTreeMap<PublicKey, U512>,
    ) {
        self.validator_matrix
            .register_validator_weights(era_id, validators);

        if let Some(validator_weights) = self.validator_matrix.validator_weights(era_id) {
            if let Some(builder) = &mut self.forward {
                if builder.needs_validators(era_id) {
                    builder.register_era_validator_weights(validator_weights.clone());
                }
            }
            if let Some(builder) = &mut self.historical {
                if builder.needs_validators(era_id) {
                    builder.register_era_validator_weights(validator_weights);
                }
            }
        }
    }

    // NOT WIRED OR EVENTED
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

    // NOT WIRED OR EVENTED
    pub(crate) fn flush_dishonest_peers(&mut self) {
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
                .set_timeout(Duration::from_millis(NEED_NEXT_INTERVAL_MILLIS))
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
        REv: ReactorEvent,
    {
        let mut results = Effects::new();
        let mut builder_needs_next = |builder: &mut BlockBuilder| {
            let action = builder.block_acquisition_action(rng);
            let peers = action.peers_to_ask(); // pass this to any fetcher
            match action.need_next() {
                NeedNext::Nothing => {}
                NeedNext::BlockHeader(block_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockHeader>(block_hash, node_id, ())
                            .event(Event::BlockHeaderFetched)
                    }))
                }
                NeedNext::BlockBody(block_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<ExecutedBlock>(block_hash, node_id, ())
                            .event(Event::BlockAddedFetched)
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
                                .fetch::<FinalitySignature>(id, node_id, ())
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
                NeedNext::ExecutionResultsRootHash {
                    block_hash,
                    global_state_root_hash,
                } => {
                    results.extend(
                        effect_builder
                            .get_execution_results_root_hash(global_state_root_hash)
                            .event(move |result| Event::GotExecutionResultsRootHash {
                                block_hash,
                                result,
                            }),
                    );
                }
                NeedNext::Deploy(block_hash, deploy_hash_or_id) => match deploy_hash_or_id {
                    Either::Left(deploy_hash) => {
                        results.extend(peers.into_iter().flat_map(|node_id| {
                            effect_builder
                                .fetch::<LegacyDeploy>(deploy_hash, node_id, ())
                                .event(move |result| Event::DeployFetched {
                                    block_hash,
                                    result: Either::Left(result),
                                })
                        }))
                    }
                    Either::Right(deploy_id) => {
                        results.extend(peers.into_iter().flat_map(|node_id| {
                            effect_builder
                                .fetch::<Deploy>(deploy_id, node_id, ())
                                .event(move |result| Event::DeployFetched {
                                    block_hash,
                                    result: Either::Right(result),
                                })
                        }))
                    }
                },
                NeedNext::ExecutionResults(block_hash, id) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockExecutionResultsOrChunk>(id, node_id, ())
                            .event(move |result| Event::ExecutionResultsFetched {
                                block_hash,
                                result,
                            })
                    }))
                }
                NeedNext::EraValidators(era_id) => results.extend(
                    effect_builder
                        .get_era_validators(era_id)
                        .event(move |maybe| Event::MaybeEraValidators(era_id, maybe)),
                ),
                NeedNext::Peers(block_hash) => results.extend(
                    effect_builder
                        .get_blocks_accumulated_peers(block_hash)
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

    fn disconnect_from_peer(&mut self, node_id: NodeId) -> Effects<Event> {
        if let Some(builder) = &mut self.forward {
            builder.demote_peer(Some(node_id));
        }
        if let Some(builder) = &mut self.historical {
            builder.demote_peer(Some(node_id));
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
                match err {
                    Error::Absent { id, peer }
                    | Error::Rejected { id, peer }
                    | Error::TimedOut { id, peer } => (id, None, Some(peer)),
                    Error::CouldNotConstructGetRequest { id, .. } => (id, None, None),
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

    fn block_added_fetched(
        &mut self,
        result: Result<FetchedData<ExecutedBlock>, Error<ExecutedBlock>>,
    ) -> Effects<Event> {
        let (block_hash, maybe_block_added, maybe_peer_id): (
            BlockHash,
            Option<Box<ExecutedBlock>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item.block().id(), Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item.block().id(), Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch block-added");
                match err {
                    Error::Absent { id, peer }
                    | Error::Rejected { id, peer }
                    | Error::TimedOut { id, peer } => (id, None, Some(peer)),
                    Error::CouldNotConstructGetRequest { id, .. } => (id, None, None),
                }
            }
        };

        match (&mut self.forward, &mut self.historical) {
            (Some(builder), _) | (_, Some(builder)) if builder.block_hash() == block_hash => {
                match maybe_block_added {
                    None => {
                        builder.demote_peer(maybe_peer_id);
                    }
                    Some(block_added) => {
                        if let Err(error) =
                            builder.register_block_added(&block_added, maybe_peer_id)
                        {
                            error!(%error, "failed to apply block-added");
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
                match err {
                    Error::Absent { id, peer }
                    | Error::Rejected { id, peer }
                    | Error::TimedOut { id, peer } => (id, None, Some(peer)),
                    Error::CouldNotConstructGetRequest { id, .. } => (id, None, None),
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
            Ok(Some(digest)) => ExecutionResultsChecksum::Checkable(digest),
            Ok(None) => {
                error!("the checksum registry should contain the execution results checksum");
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

    fn execution_results_fetched<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_hash: BlockHash,
        result: FetchResult<BlockExecutionResultsOrChunk>,
    ) -> Effects<Event>
    where
        REv: From<StorageRequest> + Send,
    {
        let (value_or_chunk, maybe_peer) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item, Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item, None),
            Err(err) => {
                debug!(%err, "failed to fetch execution results or chunk");
                // TODO: Remove peer?
                return Effects::new();
            }
        };

        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "not currently synchronizing block");
                return Effects::new();
            }

            // do to reasons, the stiched back together execution effects need to be saved to disk
            // here, when the last chunk is collected.
            // we expect a response back, which will crank the block builder for this block to the
            // next state.
            match builder.register_fetched_execution_results(maybe_peer, *value_or_chunk) {
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
        Effects::new()
    }

    fn execution_results_stored_notification(&mut self, block_hash: BlockHash) -> Effects<Event> {
        if let Some(builder) = &mut self.historical {
            if builder.block_hash() != block_hash {
                debug!(%block_hash, "not currently synchronizing block");
            } else if let Err(error) = builder.register_execution_results_stored_notification() {
                error!(%block_hash, %error, "failed to apply stored execution results");
            }
        }
        Effects::new()
    }

    // // NOT WIRED OR EVENTED
    // fn stop(&mut self, block_hash: &BlockHash) {
    //     if let Some(builder) = &mut self.fwd {
    //         builder.abort();
    //     }
    //     if let Some(builder) = &mut self.historical {
    //         builder.abort();
    //     }
    // }
    //
    // // NOT WIRED OR EVENTED
    // fn flush(&mut self) {
    //     self.fwd = None;
    //     self.historical = None;
    // }
}

const NEED_NEXT_INTERVAL_MILLIS: u64 = 30;

impl<REv> Component<REv> for BlockSynchronizer
where
    REv: ReactorEvent,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        if self.disabled {
            warn!(%event, "block synchronizer not currently enabled - ignoring event");
            return Effects::new();
        }

        // MISSING EVENT: ANNOUNCEMENT OF BAD PEERS
        match event {
            Event::Request(BlockSynchronizerRequest::NeedNext) => {
                self.need_next(effect_builder, rng)
            }
            // triggered indirectly via need next effects, and they perpetuate
            // another need next to keep it going
            Event::BlockHeaderFetched(result) => {
                let effects = self.block_header_fetched(result);
                self.hook_need_next(effect_builder, effects)
            }
            Event::BlockAddedFetched(result) => {
                let effects = self.block_added_fetched(result);
                self.hook_need_next(effect_builder, effects)
            }
            Event::FinalitySignatureFetched(result) => {
                let effects = self.finality_signature_fetched(result);
                self.hook_need_next(effect_builder, effects)
            }
            Event::GlobalStateSynced { block_hash, result } => {
                let effects = self.global_state_synced(block_hash, result);
                self.hook_need_next(effect_builder, effects)
            }
            Event::GotExecutionResultsRootHash { block_hash, result } => {
                let effects = self.got_execution_results_root_hash(block_hash, result);
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
            Event::ExecutionResultsFetched { block_hash, result } => {
                let effects = self.execution_results_fetched(effect_builder, block_hash, result);
                self.hook_need_next(effect_builder, effects)
            }
            Event::ExecutionResultsStored(block_hash) => {
                let effects = self.execution_results_stored_notification(block_hash);
                self.hook_need_next(effect_builder, effects)
            }
            Event::AccumulatedPeers(block_hash, Some(peers)) => {
                let effects = self.register_peers(block_hash, peers);
                self.hook_need_next(effect_builder, effects)
            }
            Event::AccumulatedPeers(_, None) => self.hook_need_next(effect_builder, Effects::new()),
            // this is an explicit ask via need_next for validators if they are
            // missing for a given era
            Event::MaybeEraValidators(era_id, Some(era_validators)) => {
                self.register_needed_era_validators(era_id, era_validators);
                self.hook_need_next(effect_builder, Effects::new())
            }
            Event::MaybeEraValidators(_, None) => {
                self.hook_need_next(effect_builder, Effects::new())
            }

            // CURRENTLY NOT TRIGGERED -- should get this from step announcement
            Event::EraValidators {
                era_validator_weights: validators,
            } => {
                self.validator_matrix
                    .register_era_validator_weights(validators);
                Effects::new()
            }

            Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                Event::GlobalStateSynchronizer,
                self.global_sync.handle_event(effect_builder, rng, event),
            ),

            // TRIGGERED VIA ANNOUNCEMENT
            Event::DisconnectFromPeer(node_id) => self.disconnect_from_peer(node_id),
        }
    }
}
