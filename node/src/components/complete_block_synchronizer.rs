mod complete_block_builder;
mod config;
mod event;

use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap},
    convert::Infallible,
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
    effect::{
        announcements::ControlLogicAnnouncement, requests::FetcherRequest, EffectBuilder,
        EffectExt, Effects,
    },
    storage::StorageRequest,
    types::{Block, BlockAdded, BlockHash, Deploy, FinalitySignature, FinalitySignatureId, NodeId},
    NodeRng,
};

use complete_block_builder::{BlockAcquisitionState, CompleteBlockBuilder, NeedNext};
pub(crate) use config::Config;
pub(crate) use event::Event;

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub(crate) struct CompleteBlockSyncRequest {
    pub(crate) block_hash: BlockHash,
    pub(crate) era_id: EraId,
    pub(crate) should_fetch_execution_state: bool,
    pub(crate) peer: NodeId,
}

impl Display for CompleteBlockSyncRequest {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "complete block ID {} with should fetch execution state: {} from peer {}",
            self.block_hash, self.should_fetch_execution_state, self.peer
        )
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct CompleteBlockSynchronizer {
    timeout: TimeDiff,
    #[data_size(skip)]
    fault_tolerance_fraction: Ratio<u64>,
    builders: HashMap<BlockHash, CompleteBlockBuilder>,
    validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
}

impl CompleteBlockSynchronizer {
    pub(crate) fn new(config: Config, fault_tolerance_fraction: Ratio<u64>) -> Self {
        CompleteBlockSynchronizer {
            timeout: config.timeout(),
            fault_tolerance_fraction,
            builders: Default::default(),
            validators: Default::default(),
        }
    }

    fn upsert<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request: CompleteBlockSyncRequest,
    ) -> Effects<Event>
    where
        REv: From<ControlLogicAnnouncement> + From<FetcherRequest<Block>> + Send,
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
                        return effect_builder
                            .control_announce_missing_validator_set(request.era_id)
                            .ignore();
                    }
                    Some(validators) => validators.clone(),
                };
                let builder = CompleteBlockBuilder::new(
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

impl CompleteBlockSynchronizer {
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
        REv: From<FetcherRequest<Block>>
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
                            .fetch::<Block>(block_hash, node_id, ())
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

    fn handle_block_fetched(
        &mut self,
        result: Result<FetchedData<Block>, fetcher::Error<Block>>,
    ) -> Effects<Event> {
        let block = match result {
            Ok(FetchedData::FromPeer { item, peer: _ } | FetchedData::FromStorage { item }) => item,
            Err(err) => {
                debug!(%err, "failed to fetch block");
                // TODO: Remove peer?
                return Effects::new();
            }
        };

        match self.builders.get_mut(block.hash()) {
            Some(builder) => builder.apply_block(&block),
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

impl<REv> Component<REv> for CompleteBlockSynchronizer
where
    REv: From<ControlLogicAnnouncement>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalitySignature>>
        + From<StorageRequest>
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
        match event {
            Event::EraValidators { mut validators } => {
                self.validators.append(&mut validators);
                Effects::new()
            }
            Event::Upsert(request) => self.upsert(effect_builder, request),
            Event::Next => self.next(effect_builder),
            Event::DisconnectFromPeer(node_id) => self.handle_disconnect_from_peer(node_id),
            Event::BlockFetched(result) => self.handle_block_fetched(result),
            Event::FinalitySignatureFetched(result) => {
                self.handle_finality_signature_fetched(result)
            }
            _ => todo!(),
        }
    }
}
