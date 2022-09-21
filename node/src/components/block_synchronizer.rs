mod block_acquisition;
mod block_builder;
mod config;
mod deploy_acquisition;
mod event;
mod global_state_synchronizer;
mod need_next;
mod peer_list;
mod signature_acquisition;
mod trie_accumulator;

use std::collections::hash_map::ValuesMut;
use std::rc::Rc;
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

use crate::components::fetcher::Error;

use crate::effect::announcements::BlocklistAnnouncement;
use crate::effect::requests::{ContractRuntimeRequest, TrieAccumulatorRequest};
use crate::types::{Item, TrieOrChunk, ValidatorMatrix};
pub(crate) use block_builder::BlockBuilder;
pub(crate) use config::Config;
pub(crate) use event::Event;
use global_state_synchronizer::GlobalStateSynchronizer;
pub(crate) use global_state_synchronizer::{
    Error as GlobalStateSynchronizerError, Event as GlobalStateSynchronizerEvent,
};
pub(crate) use need_next::NeedNext;
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
    global_sync: GlobalStateSynchronizer,
}

impl BlockSynchronizer {
    pub(crate) fn new(config: Config, fault_tolerance_fraction: Ratio<u64>) -> Self {
        BlockSynchronizer {
            timeout: config.timeout(),
            fault_tolerance_fraction,
            builders: Default::default(),
            validators: Default::default(),
            last_progress: None,
            global_sync: GlobalStateSynchronizer::new(config.max_parallel_trie_fetches() as usize),
        }
    }

    // When was progress last made (if any).
    pub(crate) fn last_progress(&self) -> Option<Timestamp> {
        self.builders
            .values()
            .filter_map(BlockBuilder::last_progress_time)
            .chain(self.global_sync.last_progress_timestamp().into_iter())
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
                entry.get_mut().register_peer(request.peer);
            }
            Entry::Vacant(entry) => {
                let era_id = request.era_id;
                let mut validator_matrix = ValidatorMatrix::new(self.fault_tolerance_fraction);
                match self.validators.get(&era_id) {
                    None => {
                        debug!(
                            era_id = %era_id,
                            "missing validators for given era"
                        );
                    }
                    Some(validators) => validator_matrix.register_era(era_id, validators.clone()),
                };

                entry.insert(BlockBuilder::new(
                    request.block_hash,
                    era_id,
                    Rc::new(validator_matrix),
                    request.should_fetch_execution_state,
                ));
            }
        }
        Effects::new()
    }
}

impl BlockSynchronizer {
    fn register_peer(&mut self, block_hash: &BlockHash, peer: NodeId) {
        if let Some(builder) = self.builders.get_mut(block_hash) {
            builder.register_peer(peer)
        }
    }

    fn next<REv>(&mut self, effect_builder: EffectBuilder<REv>, rng: &mut NodeRng) -> Effects<Event>
    where
        REv: From<FetcherRequest<BlockAdded>>
            + From<FetcherRequest<Deploy>>
            + From<FetcherRequest<FinalitySignature>>
            + Send,
    {
        let mut results = Effects::new();

        for builder in self.builders.values_mut() {
            let (peers, next) = builder.next_needed(rng).build();
            match next {
                // No further parts of the block are missing. Nothing to do.
                NeedNext::Nothing => {}
                NeedNext::BlockHeader(block_hash) => todo!(),
                NeedNext::BlockBody(block_hash) => {
                    // TODO - change to fetch block/block-body
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockAdded>(block_hash, node_id, ())
                            .event(Event::BlockAddedFetched)
                    }))
                }
                NeedNext::FinalitySignatures(block_hash, era_id, validators) => {
                    // TODO - fetch all signatures
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
                NeedNext::GlobalState(_) => todo!(),
                NeedNext::Deploy(deploy_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Deploy>(deploy_hash, node_id, ())
                            .event(Event::DeployFetched)
                    }))
                }
                NeedNext::ExecutionResults(block_hash) => todo!(),
                NeedNext::EraValidators(era_id) => todo!(),
                // We expect to be told about new peers automatically; do nothing.
                NeedNext::Peers => {}
            }
        }
        results
    }

    fn handle_disconnect_from_peer(&mut self, node_id: NodeId) -> Effects<Event> {
        for builder in self.builders.values_mut() {
            builder.demote_peer(Some(node_id));
        }
        Effects::new()
    }

    fn handle_block_added_fetched(
        &mut self,
        result: Result<FetchedData<BlockAdded>, Error<BlockAdded>>,
    ) -> Effects<Event> {
        let (block_hash, maybe_block_added, maybe_peer_id): (
            BlockHash,
            Option<Box<BlockAdded>>,
            Option<NodeId>,
        ) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item.block.id(), Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item.block.id(), Some(item), None),
            Err(err) => {
                debug!(%err, "failed to fetch block-added");
                match err {
                    Error::Absent { id, peer }
                    | Error::Rejected { id, peer }
                    | Error::TimedOut { id, peer } => {
                        (id, None::<Option<Box<BlockAdded>>>, Some(peer))
                    }
                    Error::CouldNotConstructGetRequest { id, .. } => (id, None, None),
                };
                return Effects::new();
            }
        };

        let builder = match self.builders.get_mut(&block_hash) {
            Some(builder) => builder,
            None => {
                debug!("unexpected block added");
                return Effects::new();
            }
        };

        match maybe_block_added {
            None => builder.demote_peer(maybe_peer_id),
            Some(block_added) => match builder.apply_block(&block_added, maybe_peer_id) {
                Ok(_) => {}
                Err(err) => {
                    error!(%err, "failed to apply block-added");
                }
            },
        }

        Effects::new()
    }

    fn handle_finality_signature_fetched(
        &mut self,
        result: Result<FetchedData<FinalitySignature>, Error<FinalitySignature>>,
    ) -> Effects<Event> {
        let (finality_signature, maybe_peer) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item, Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item, None),
            Err(err) => {
                debug!(%err, "failed to fetch finality signature");
                // TODO: Remove peer?
                return Effects::new();
            }
        };

        match self.builders.get_mut(&finality_signature.block_hash) {
            Some(builder) => builder.apply_finality_signature(*finality_signature, maybe_peer),
            None => {
                debug!("unexpected block");
                return Effects::new();
            }
        };
        Effects::new()
    }

    /// Reactor instructing this instance to be stopped
    fn stop(&mut self, block_hash: &BlockHash) {
        match self.builders.get_mut(block_hash) {
            None => {
                // noop
            }
            Some(builder) => {
                let _ = builder.abort();
            }
        }
    }

    fn flush(&mut self) {
        self.builders
            .retain(|k, v| (v.is_fatal() || v.is_complete()) == false);
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
            Event::Next => self.next(effect_builder, rng),
            Event::DisconnectFromPeer(node_id) => self.handle_disconnect_from_peer(node_id),
            Event::BlockAddedFetched(result) => self.handle_block_added_fetched(result),
            Event::FinalitySignatureFetched(result) => {
                self.handle_finality_signature_fetched(result)
            }
            Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                Event::GlobalStateSynchronizer,
                self.global_sync.handle_event(effect_builder, rng, event),
            ),
            _ => todo!(),
        }
    }
}
