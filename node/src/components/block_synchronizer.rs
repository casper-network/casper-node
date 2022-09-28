mod block_acquisition;
mod block_builder;
mod config;
mod deploy_acquisition;
mod event;
mod execution_results_accumulator;
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

use crate::components::block_synchronizer::block_acquisition::BlockAcquisitionAction;
use crate::contract_runtime::EraValidatorsRequest;
use crate::effect::requests::{
    BlocksAccumulatorRequest, ChainspecRawBytesRequest, SyncGlobalStateRequest,
};
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
use casper_hashing::Digest;
pub(crate) use config::Config;
pub(crate) use event::Event;
use execution_results_accumulator::ExecutionResultsAccumulator;
pub(crate) use execution_results_accumulator::{
    Error as ExecutionResultsAccumulatorError, Event as ExecutionResultsAccumulatorEvent,
};
use global_state_synchronizer::GlobalStateSynchronizer;
pub(crate) use global_state_synchronizer::{
    Error as GlobalStateSynchronizerError, Event as GlobalStateSynchronizerEvent,
};
pub(crate) use need_next::NeedNext;
use trie_accumulator::TrieAccumulator;
pub(crate) use trie_accumulator::{Error as TrieAccumulatorError, Event as TrieAccumulatorEvent};

// #[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
// pub(crate) struct BlockSyncRequest {
//     block_hash: BlockHash,
//     era_id: EraId,
//     should_fetch_execution_state: bool,
//     max_simultaneous_peers: u32,
// }
//
// impl Display for BlockSyncRequest {
//     fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
//         write!(
//             formatter,
//             "complete block ID {} with should fetch execution state: {}",
//             self.block_hash, self.should_fetch_execution_state
//         )
//     }
// }

#[derive(DataSize, Debug)]
pub(crate) struct BlockSynchronizer {
    timeout: TimeDiff,
    builders: HashMap<BlockHash, BlockBuilder>,
    validator_matrix: ValidatorMatrix,
    last_progress: Timestamp,
    global_sync: GlobalStateSynchronizer,
}

impl BlockSynchronizer {
    pub(crate) fn new(config: Config, fault_tolerance_fraction: Ratio<u64>) -> Self {
        BlockSynchronizer {
            timeout: config.timeout(),
            builders: Default::default(),
            validator_matrix: Default::default(),
            last_progress: Timestamp::now(),
            global_sync: GlobalStateSynchronizer::new(config.max_parallel_trie_fetches() as usize),
        }
    }

    // CALLED FROM REACTOR
    pub(crate) fn highest_executable_block_hash(&self) -> Option<BlockHash> {
        self.builders
            .iter()
            .filter(|(k, v)| v.is_complete())
            .filter(|(k, v)| v.block_height().is_some())
            .max_by(|x, y| x.1.block_height().cmp(&y.1.block_height()))
            .map(|(k, _)| *k)
    }

    // CALLED FROM REACTOR
    pub(crate) fn register_block_by_hash(
        &mut self,
        block_hash: BlockHash,
        should_fetch_execution_state: bool,
        max_simultaneous_peers: u32,
    ) {
        if self.builders.get_mut(&block_hash).is_none() {
            self.last_progress = Timestamp::now();
            self.builders.insert(
                block_hash,
                BlockBuilder::new(
                    block_hash,
                    should_fetch_execution_state,
                    max_simultaneous_peers,
                ),
            );
        }
    }

    // CALLED FROM REACTOR
    pub(crate) fn last_progress(&self) -> Timestamp {
        self.builders
            .values()
            .filter_map(BlockBuilder::last_progress_time)
            .chain(std::iter::once(self.global_sync.last_progress()))
            .chain(std::iter::once(self.last_progress))
            .max()
            .unwrap_or(self.last_progress)
    }

    // NOT WIRED OR EVENTED
    fn register_sync_leap(
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
                builder.register_finality_signature(
                    FinalitySignature::new(block_hash, era_id, sig, public_key),
                    None,
                );
            }
        }

        if sync_leap.apply_validator_weights(&mut self.validator_matrix) {
            self.last_progress = Timestamp::now();
        }
        if let Some(fat_block_header) = sync_leap.highest_block_header() {
            match self.builders.get_mut(&block_hash) {
                Some(builder) => {
                    apply_sigs(builder, fat_block_header);
                }
                None => {
                    let era_id = fat_block_header.block_header.era_id();
                    if let Some(vw) = self.validator_matrix.validator_weights(era_id) {
                        let validator_weights = vw;
                        let mut builder = BlockBuilder::new_from_sync_leap(
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

    // WIRED & EVENTED
    fn register_peers(&mut self, block_hash: BlockHash, peers: Vec<NodeId>) -> Effects<Event> {
        if let Some(builder) = self.builders.get_mut(&block_hash) {
            for peer in peers {
                builder.register_peer(peer);
            }
        }
        Effects::new()
    }

    // WIRED & EVENTED
    fn register_needed_era_validators(
        &mut self,
        era_id: EraId,
        validators: BTreeMap<PublicKey, U512>,
    ) {
        self.validator_matrix
            .register_validator_weights(era_id, validators);

        if let Some(validator_weights) = self.validator_matrix.validator_weights(era_id) {
            for builder in self.builders.values_mut() {
                if builder.needs_validators(era_id) {
                    builder.register_era_validator_weights(validator_weights.clone());
                }
            }
        }
    }

    // NOT WIRED OR EVENTED
    fn dishonest_peers(&self) -> Vec<NodeId> {
        let mut ret = vec![];
        for v in self.builders.values() {
            ret.extend(v.dishonest_peers());
        }
        ret
    }

    // NOT WIRED OR EVENTED
    pub(crate) fn flush_dishonest_peers(&mut self) {
        for v in self.builders.values_mut() {
            v.flush_dishonest_peers();
        }
    }

    // fn upsert(&mut self, request: BlockSyncRequest) {
    //     if let Entry::Vacant(v) = self.builders.entry(request.block_hash) {
    //         let validator_matrix = &self.validator_matrix;
    //         match validator_matrix.validator_weights(request.era_id) {
    //             Some(validator_weights) => {
    //                 let builder = BlockBuilder::new(
    //                     request.block_hash,
    //                     request.era_id,
    //                     validator_weights,
    //                     request.should_fetch_execution_state,
    //                     request.max_simultaneous_peers,
    //                 );
    //                 v.insert(builder);
    //             }
    //             None => {}
    //         }
    //     }
    // }

    // NOT WIRED
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
            + From<SyncGlobalStateRequest>
            + From<BlocksAccumulatorRequest>
            + From<ChainspecRawBytesRequest>
            + From<StorageRequest>
            + Send,
    {
        let mut results = Effects::new();

        for (block_hash, builder) in &mut self.builders {
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
                NeedNext::GlobalState(block_hash, global_state_root_hash) => results.extend(
                    effect_builder
                        .sync_global_state(
                            block_hash,
                            global_state_root_hash,
                            peers.into_iter().collect(),
                        )
                        .event(move |result| Event::GlobalStateSynced { block_hash, result }),
                ),
                NeedNext::Deploy(block_hash, deploy_hash) => {
                    results.extend(peers.into_iter().flat_map(|node_id| {
                        effect_builder
                            .fetch::<Deploy>(deploy_hash, node_id, ())
                            .event(move |result| Event::DeployFetched { block_hash, result })
                    }))
                }
                NeedNext::ExecutionResults(block_hash) => todo!(),
                NeedNext::EraValidators(era_id) => results.extend(
                    effect_builder
                        .get_era_validators(era_id)
                        .event(move |maybe| Event::MaybeEraValidators(era_id, maybe)),
                ),
                NeedNext::Peers(block_hash) => results.extend(
                    effect_builder
                        .get_block_accumulated_peers(block_hash)
                        .event(Event::AccumulatedPeers),
                ),
            }
        }
        results
    }

    // NOT WIRED
    fn disconnect_from_peer(&mut self, node_id: NodeId) -> Effects<Event> {
        for builder in self.builders.values_mut() {
            builder.demote_peer(Some(node_id));
        }
        Effects::new()
    }

    // NOT WIRED
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
            Some(block_header) => {
                if let Err(error) = builder.register_block_header(*block_header, maybe_peer_id) {
                    error!(%error, "failed to apply block header");
                }
            }
        };

        Effects::new()
    }

    // NOT WIRED OR EVENTED
    // fn block_fetched(
    //     &mut self,
    //     result: Result<FetchedData<BlockAdded>, Error<BlockAdded>>,
    // ) -> Effects<Event> {
    //     let (block_hash, maybe_block, maybe_peer_id): (
    //         BlockHash,
    //         Option<Box<BlockAdded>>,
    //         Option<NodeId>,
    //     ) = match result {
    //         Ok(FetchedData::FromPeer { item, peer }) => (item.id(), Some(item), Some(peer)),
    //         Ok(FetchedData::FromStorage { item }) => (item.id(), Some(item), None),
    //         Err(err) => {
    //             debug!(%err, "failed to fetch block-added");
    //             match err {
    //                 Error::Absent { id, peer }
    //                 | Error::Rejected { id, peer }
    //                 | Error::TimedOut { id, peer } => (id, None, Some(peer)),
    //                 Error::CouldNotConstructGetRequest { id, .. } => (id, None, None),
    //             }
    //         }
    //     };
    //
    //     let builder = match self.builders.get_mut(&block_hash) {
    //         Some(builder) => builder,
    //         None => {
    //             debug!("unexpected block");
    //             return Effects::new();
    //         }
    //     };
    //
    //     match maybe_block {
    //         None => {
    //             builder.demote_peer(maybe_peer_id);
    //         }
    //         Some(block) => {
    //             if let Err(error) = builder.register_block_added(&block, maybe_peer_id) {
    //                 error!(%error, "failed to apply block");
    //             }
    //         }
    //     };
    //
    //     Effects::new()
    // }

    // NOT WIRED
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
            Some(block_added) => match builder.register_block_added(&block_added, maybe_peer_id) {
                Ok(_) => {}
                Err(err) => {
                    error!(%err, "failed to apply block-added");
                }
            },
        };

        Effects::new()
    }

    // NOT WIRED
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
            Some(builder) => {
                if let Err(error) =
                    builder.register_finality_signature(*finality_signature, maybe_peer)
                {
                    error!(%error, "failed to apply finality signature");
                }
            }
            None => {
                debug!(
                    block_hash=%finality_signature.block_hash,
                    "not currently synchronising block");
                return Effects::new();
            }
        };
        Effects::new()
    }

    // NOT WIRED
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

        match self.builders.get_mut(&block_hash) {
            Some(builder) => {
                if let Err(error) = builder.register_global_state(root_hash) {
                    error!(%block_hash, %error, "failed to apply global state");
                }
            }
            None => debug!(%block_hash, "not currently synchronising block"),
        }

        Effects::new()
    }

    // NOT WIRED
    fn deploy_fetched(
        &mut self,
        block_hash: BlockHash,
        result: Result<FetchedData<Deploy>, Error<Deploy>>,
    ) -> Effects<Event> {
        let (deploy, maybe_peer) = match result {
            Ok(FetchedData::FromPeer { item, peer }) => (item, Some(peer)),
            Ok(FetchedData::FromStorage { item }) => (item, None),
            Err(err) => {
                debug!(%err, "failed to fetch deploy");
                // TODO: Remove peer?
                return Effects::new();
            }
        };

        match self.builders.get_mut(&block_hash) {
            Some(builder) => {
                if let Err(error) = builder.register_deploy(*deploy.id(), maybe_peer) {
                    error!(%block_hash, %error, "failed to apply deploy");
                }
            }
            None => debug!(%block_hash, "not currently synchronizing block"),
        };
        Effects::new()
    }

    // NOT WIRED OR EVENTED
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

    // NOT WIRED OR EVENTED
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
        + From<BlocksAccumulatorRequest>
        + From<PeerBehaviorAnnouncement>
        + From<StorageRequest>
        + From<TrieAccumulatorRequest>
        + From<ContractRuntimeRequest>
        + From<SyncGlobalStateRequest>
        + From<ChainspecRawBytesRequest>
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
            // TRIGGERED
            Event::DisconnectFromPeer(node_id) => self.disconnect_from_peer(node_id),

            // NOT TRIGGERED
            Event::EraValidators {
                era_validator_weights: validators,
            } => {
                self.validator_matrix
                    .register_era_validator_weights(validators);
                Effects::new()
            }
            // Event::Upsert(request) => {
            //     self.upsert(request);
            //     Effects::new()
            // }
            // NOT TRIGGERED
            Event::Next => self.need_next(effect_builder, rng),

            // NOT TRIGGERED
            Event::BlockHeaderFetched(result) => self.block_header_fetched(result),
            // NOT TRIGGERED
            Event::BlockAddedFetched(result) => self.block_added_fetched(result),
            // NOT TRIGGERED
            Event::FinalitySignatureFetched(result) => self.finality_signature_fetched(result),
            // NOT TRIGGERED
            Event::GlobalStateSynced { block_hash, result } => {
                self.global_state_synced(block_hash, result)
            }
            // NOT TRIGGERED
            Event::DeployFetched { block_hash, result } => self.deploy_fetched(block_hash, result),
            // NOT TRIGGERED
            Event::AccumulatedPeers(Some((block_hash, peers))) => {
                self.register_peers(block_hash, peers)
            }
            Event::AccumulatedPeers(None) => Effects::new(),
            // NOT TRIGGERED
            Event::GlobalStateSynchronizer(event) => reactor::wrap_effects(
                Event::GlobalStateSynchronizer,
                self.global_sync.handle_event(effect_builder, rng, event),
            ),
            Event::MaybeEraValidators(era_id, Some(era_validators)) => {
                self.register_needed_era_validators(era_id, era_validators);
                Effects::new()
            }
            Event::MaybeEraValidators(_, None) => Effects::new(),
        }
    }
}
