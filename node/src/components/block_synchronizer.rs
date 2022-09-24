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

use std::{
    collections::{
        hash_map::{Entry, ValuesMut},
        BTreeMap, HashMap,
    },
    fmt::{self, Display, Formatter},
    rc::Rc,
};

use datasize::DataSize;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

use casper_types::{EraId, PublicKey, TimeDiff, Timestamp, U512};

use crate::{
    components::{
        fetcher::{self, FetchedData},
        Component,
    },
    effect::{requests::FetcherRequest, EffectBuilder, EffectExt, Effects},
    reactor,
    storage::StorageRequest,
    types::{
        BlockAdded, BlockHash, BlockHeader, Deploy, FinalitySignature, FinalitySignatureId, NodeId,
    },
    NodeRng,
};

use crate::components::fetcher::Error;

use crate::{
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{ContractRuntimeRequest, TrieAccumulatorRequest},
    },
    types::{
        Block, BlockHeaderWithMetadata, EraValidatorWeights, Item, SyncLeap, TrieOrChunk,
        ValidatorMatrix,
    },
};
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
    block_hash: BlockHash,
    era_id: EraId,
    should_fetch_execution_state: bool,
    max_simultaneous_peers: u32,
}

impl Display for BlockSyncRequest {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "complete block ID {} with should fetch execution state: {}",
            self.block_hash, self.should_fetch_execution_state
        )
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockSynchronizer {
    timeout: TimeDiff,
    builders: HashMap<BlockHash, BlockBuilder>,
    validator_matrix: ValidatorMatrix,
    last_progress: Option<Timestamp>,
    global_sync: GlobalStateSynchronizer,
}

impl BlockSynchronizer {
    pub(crate) fn new(config: Config, fault_tolerance_fraction: Ratio<u64>) -> Self {
        BlockSynchronizer {
            timeout: config.timeout(),
            builders: Default::default(),
            validator_matrix: Default::default(),
            last_progress: None,
            global_sync: GlobalStateSynchronizer::new(config.max_parallel_trie_fetches() as usize),
        }
    }

    pub(crate) fn sync(
        &mut self,
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) {
        if self.builders.get_mut(&block_hash).is_none() {
            self.builders.insert(
                block_hash,
                BlockBuilder::new_minimal(
                    block_hash,
                    should_fetch_execution_state,
                    max_simultaneous_peers,
                ),
            );
        }
    }

    pub(crate) fn leap(
        &mut self,
        block_hash: BlockHash,
        sync_leap: SyncLeap,
        peers: Vec<NodeId>,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) {
        fn apply_sigs(builder: &mut BlockBuilder, fat_block_header: BlockHeaderWithMetadata) {
            let block_hash = builder.block_hash();
            let era_id = fat_block_header.block_header.era_id();
            for (public_key, sig) in fat_block_header.block_signatures.proofs {
                builder.apply_finality_signature(
                    FinalitySignature::new(block_hash, era_id, sig, public_key),
                    None,
                );
            }
        }

        sync_leap.apply_validator_weights(&mut self.validator_matrix);
        if let Some(fat_block_header) = sync_leap.highest_block_header() {
            match self.builders.get_mut(&block_hash) {
                Some(builder) => {
                    apply_sigs(builder, fat_block_header);
                }
                None => {
                    let era_id = fat_block_header.block_header.era_id();
                    if let Some(vw) = self.validator_matrix.validator_weights(era_id) {
                        let validator_weights = vw;
                        let mut builder = BlockBuilder::new_leap(
                            block_hash,
                            sync_leap,
                            validator_weights,
                            peers,
                            self.validator_matrix.fault_tolerance_threshold(),
                            should_fetch_execution_state,
                            max_simultaneous_peers,
                        );
                        apply_sigs(&mut builder, fat_block_header);
                        self.builders.insert(block_hash, builder);
                    } else {
                        warn!(
                            "unable to create block builder for block_hash: {}",
                            block_hash
                        );
                    }
                }
            };
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

    fn register_peer(&mut self, block_hash: &BlockHash, peer: NodeId) {
        if let Some(builder) = self.builders.get_mut(block_hash) {
            builder.register_peer(peer)
        }
    }

    fn upsert(&mut self, request: BlockSyncRequest) {
        if let Entry::Vacant(v) = self.builders.entry(request.block_hash) {
            let validator_matrix = &self.validator_matrix;
            match validator_matrix.validator_weights(request.era_id) {
                Some(validator_weights) => {
                    let builder = BlockBuilder::new(
                        request.block_hash,
                        request.era_id,
                        validator_weights,
                        request.should_fetch_execution_state,
                        request.max_simultaneous_peers,
                    );
                    v.insert(builder);
                }
                None => {}
            }
        }
    }

    fn need_next<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<BlockAdded>>
            + From<FetcherRequest<BlockHeader>>
            + From<FetcherRequest<Deploy>>
            + From<FetcherRequest<FinalitySignature>>
            + Send,
    {
        let mut results = Effects::new();

        for builder in self.builders.values_mut() {
            let (peers, next) = builder.need_next(rng).build();
            match next {
                // No further parts of the block are missing. Nothing to do.
                NeedNext::Nothing => {}
                NeedNext::BlockHeader(block_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<BlockHeader>(block_hash, node_id, ())
                            .event(Event::BlockHeaderFetched)
                    }))
                }
                NeedNext::BlockBody(block_hash) => {
                    // TODO - change to fetch block/block-body
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

    fn disconnect_from_peer(&mut self, node_id: NodeId) -> Effects<Event> {
        for builder in self.builders.values_mut() {
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

        let builder = match self.builders.get_mut(&block_hash) {
            Some(builder) => builder,
            None => {
                debug!("unexpected block header");
                return Effects::new();
            }
        };

        match maybe_block_header {
            None => {
                builder.demote_peer(maybe_peer_id);
            }
            Some(block_header) => match builder.apply_header(*block_header, maybe_peer_id) {
                Ok(_) => {}
                Err(err) => {
                    error!(%err, "failed to apply block header");
                }
            },
        };

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
            Ok(FetchedData::FromPeer { item, peer }) => (item.id(), Some(item), Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item.id(), Some(item), None),
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

        let builder = match self.builders.get_mut(&block_hash) {
            Some(builder) => builder,
            None => {
                debug!("unexpected block");
                return Effects::new();
            }
        };

        match maybe_block {
            None => {
                builder.demote_peer(maybe_peer_id);
            }
            Some(block) => match builder.apply_block(&block, maybe_peer_id) {
                Ok(_) => {}
                Err(err) => {
                    error!(%err, "failed to apply block");
                }
            },
        };

        Effects::new()
    }

    fn block_added_fetched(
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
                    | Error::TimedOut { id, peer } => (id, None, Some(peer)),
                    Error::CouldNotConstructGetRequest { id, .. } => (id, None, None),
                }
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
            None => {
                builder.demote_peer(maybe_peer_id);
            }
            Some(block_added) => match builder.apply_block(&block_added.block, maybe_peer_id) {
                Ok(_) => {}
                Err(err) => {
                    error!(%err, "failed to apply block-added");
                }
            },
        };

        Effects::new()
    }

    fn finality_signature_fetched(
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
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalitySignature>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<PeerBehaviorAnnouncement>
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
            Event::EraValidators {
                era_validator_weights: validators,
            } => {
                self.validator_matrix
                    .register_era_validator_weights(validators);
                Effects::new()
            }
            Event::Upsert(request) => {
                self.upsert(request);
                Effects::new()
            }
            Event::Next => self.need_next(effect_builder, rng),
            Event::DisconnectFromPeer(node_id) => self.disconnect_from_peer(node_id),
            Event::BlockHeaderFetched(result) => self.block_header_fetched(result),
            Event::BlockAddedFetched(result) => self.block_added_fetched(result),
            Event::FinalitySignatureFetched(result) => self.finality_signature_fetched(result),
            Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                Event::GlobalStateSynchronizer,
                self.global_sync.handle_event(effect_builder, rng, event),
            ),
            _ => todo!(),
        }
    }
}
