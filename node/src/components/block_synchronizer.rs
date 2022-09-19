mod block_builder;
mod config;
mod event;
mod global_state_synchronizer;
mod trie_accumulator;

use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap},
    fmt::{self, Display, Formatter},
};

use datasize::DataSize;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use casper_types::{EraId, PublicKey, TimeDiff, Timestamp, U512};

use crate::{
    components::{
        fetcher::{self, FetchedData},
        Component,
    },
    effect::{requests::FetcherRequest, EffectBuilder, EffectExt, Effects},
    reactor,
    storage::StorageRequest,
    types::{BlockAdded, BlockHash, Deploy, FinalitySignature, FinalitySignatureId, NodeId},
    NodeRng,
};

use crate::effect::announcements::BlocklistAnnouncement;
use crate::effect::requests::{ContractRuntimeRequest, TrieAccumulatorRequest};
use crate::types::TrieOrChunk;
use block_builder::{BlockAcquisitionState, BlockBuilder, NeedNext};
pub(crate) use config::Config;
pub(crate) use event::Event;
use global_state_synchronizer::GlobalStateSynchronizer;
pub(crate) use global_state_synchronizer::{
    Error as GlobalStateSynchronizerError, Event as GlobalStateSynchronizerEvent,
};
use trie_accumulator::TrieAccumulator;
pub(crate) use trie_accumulator::{Error as TrieAccumulatorError, Event as TrieAccumulatorEvent};

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub(crate) struct BlockSyncRequest {
    pub(crate) block_hash: BlockHash,
    pub(crate) era_id: EraId,
    pub(crate) should_fetch_execution_state: bool,
    pub(crate) peer: NodeId,
}

impl Display for BlockSyncRequest {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "complete block ID {} with should fetch execution state: {} from peer {}",
            self.block_hash, self.should_fetch_execution_state, self.peer
        )
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockSynchronizer {
    timeout: TimeDiff,
    #[data_size(skip)]
    fault_tolerance_fraction: Ratio<u64>,
    builders: HashMap<BlockHash, BlockBuilder>,
    validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    last_progress: Option<Timestamp>,
    global_state_synchronizer: GlobalStateSynchronizer,
}

impl BlockSynchronizer {
    pub(crate) fn new(config: Config, fault_tolerance_fraction: Ratio<u64>) -> Self {
        BlockSynchronizer {
            timeout: config.timeout(),
            fault_tolerance_fraction,
            builders: Default::default(),
            validators: Default::default(),
            last_progress: None,
            global_state_synchronizer: GlobalStateSynchronizer::new(
                config.max_parallel_trie_fetches() as usize,
            ),
        }
    }

    // When was progress last made (if any).
    pub(crate) fn last_progress(&self) -> Option<Timestamp> {
        self.builders
            .values()
            .filter_map(BlockBuilder::last_progress_time)
            .max()
    }

    fn upsert<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request: BlockSyncRequest,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<BlockAdded>> + Send,
    {
        match self.builders.entry(request.block_hash) {
            Entry::Occupied(mut entry) => {
                let _ = entry.get_mut().register_peer(request.peer);
            }
            Entry::Vacant(entry) => {
                let validators = match self.validators.get(&request.era_id) {
                    None => {
                        debug!(
                            era_id = %request.era_id,
                            "missing validators for given era"
                        );
                        todo!("we need validator set");
                    }
                    Some(validators) => validators.clone(),
                };
                let builder = BlockBuilder::new(
                    request.block_hash,
                    request.era_id,
                    validators,
                    request.should_fetch_execution_state,
                );
                let _ = entry.insert(builder);
                // effect_builder
                //     .fetch::<Block>(request.block_hash, request.peer)
                //     .event(Event::BlockFetched)
            }
        }
        Effects::new()
    }
}

pub(crate) enum BlockSyncState {
    Unknown,
    NotYetStarted,
    InProgress {
        started: Timestamp,
        most_recent: Timestamp,
        current_state: BlockAcquisitionState,
    },
    Completed,
}

impl BlockSynchronizer {
    fn block_state(self, block_hash: &BlockHash) -> BlockSyncState {
        match self.builders.get(block_hash) {
            None => BlockSyncState::Unknown,
            Some(builder) if builder.is_initialized() => BlockSyncState::NotYetStarted,
            Some(builder) if builder.is_complete() => BlockSyncState::Completed,
            Some(builder) => {
                let started = builder.started().unwrap_or_else(|| {
                    error!("started block should have started timestamp");
                    Timestamp::zero()
                });
                let last_progress_time = builder.last_progress_time().unwrap_or_else(|| {
                    error!("started block should have last_progress_time");
                    Timestamp::zero()
                });
                BlockSyncState::InProgress {
                    started,
                    most_recent: last_progress_time,
                    current_state: builder.builder_state(),
                }
            }
        }
    }

    fn register_peer(&mut self, block_hash: &BlockHash, peer: NodeId) -> bool {
        match self.builders.get_mut(block_hash) {
            None => false,
            Some(builder) if builder.is_complete() => false,
            Some(builder) => builder.register_peer(peer),
        }
    }

    fn next<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<FetcherRequest<BlockAdded>>
            + From<FetcherRequest<Deploy>>
            + From<FetcherRequest<FinalitySignature>>
            + Send,
    {
        let mut results = Effects::new();
        for builder in self.builders.values_mut() {
            let (peers, next) = builder.next_needed(self.fault_tolerance_fraction);
            match next {
                NeedNext::Block(block_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockAdded>(block_hash, node_id, ())
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
                NeedNext::GlobalState(_) => {}
                NeedNext::Deploy(deploy_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Deploy>(deploy_hash, node_id, ())
                            .event(Event::DeployFetched)
                    }))
                }
                NeedNext::ExecutionResults(_) => {}
                // No further parts of the block are missing. Nothing to do.
                NeedNext::Nothing => {}
                // We expect to be told about new peers automatically; do nothing.
                NeedNext::Peers => {}
            }
        }
        results
    }

    fn handle_disconnect_from_peer(&mut self, node_id: NodeId) -> Effects<Event> {
        for builder in self.builders.values_mut() {
            builder.remove_peer(node_id);
        }
        Effects::new()
    }

    fn handle_block_added_fetched(
        &mut self,
        result: Result<FetchedData<BlockAdded>, fetcher::Error<BlockAdded>>,
    ) -> Effects<Event> {
        let block_added = match result {
            Ok(FetchedData::FromPeer { item, peer: _ } | FetchedData::FromStorage { item }) => item,
            Err(err) => {
                debug!(%err, "failed to fetch block-added");
                // TODO: Remove peer?
                return Effects::new();
            }
        };

        match self.builders.get_mut(block_added.block.hash()) {
            Some(builder) => builder.apply_block(&block_added),
            None => {
                debug!("unexpected block");
                return Effects::new();
            }
        };
        Effects::new()
    }

    fn handle_finality_signature_fetched(
        &mut self,
        result: Result<FetchedData<FinalitySignature>, fetcher::Error<FinalitySignature>>,
    ) -> Effects<Event> {
        let finality_signature = match result {
            Ok(FetchedData::FromPeer { item, peer: _ } | FetchedData::FromStorage { item }) => item,
            Err(err) => {
                debug!(%err, "failed to fetch finality signature");
                // TODO: Remove peer?
                return Effects::new();
            }
        };

        match self.builders.get_mut(&finality_signature.block_hash) {
            Some(builder) => builder.apply_finality_signature(*finality_signature),
            None => {
                debug!("unexpected block");
                return Effects::new();
            }
        };
        Effects::new()
    }

    /// Reactor instructing this instance to be stopped
    fn stop(&mut self, block_hash: &BlockHash) {
        todo!();
    }
}

impl<REv> Component<REv> for BlockSynchronizer
where
    REv: From<FetcherRequest<BlockAdded>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalitySignature>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<BlocklistAnnouncement>
        + From<StorageRequest>
        + From<TrieAccumulatorRequest>
        + From<ContractRuntimeRequest>
        + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::EraValidators { mut validators } => {
                self.validators.append(&mut validators);
                Effects::new()
            }
            Event::Upsert(request) => self.upsert(effect_builder, request),
            Event::Next => self.next(effect_builder),
            Event::DisconnectFromPeer(node_id) => self.handle_disconnect_from_peer(node_id),
            Event::BlockAddedFetched(result) => self.handle_block_added_fetched(result),
            Event::FinalitySignatureFetched(result) => {
                self.handle_finality_signature_fetched(result)
            }
            Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                Event::GlobalStateSynchronizer,
                self.global_state_synchronizer
                    .handle_event(effect_builder, rng, event),
            ),
            _ => todo!(),
        }
    }
}
