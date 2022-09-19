use std::collections::hash_map::Entry;
use std::collections::HashSet;
use std::{collections::HashMap, time::Duration};

use datasize::DataSize;
use num_rational::Ratio;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, warn};

use casper_execution_engine::{core::engine_state, storage::trie::TrieRaw};
use casper_hashing::{ChunkWithProofVerificationError, Digest};

use crate::components::fetcher::FetchResult;
use crate::components::Component;
use crate::types::TrieOrChunk;
use crate::{
    components::trie_accumulator::{TrieAccumulatorError, TrieAccumulatorResult},
    effect::{
        requests::{ContractRuntimeRequest, TrieAccumulatorRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{BlockHash, BlockHashAndHeight, FetcherItem, Item, NodeId},
    NodeRng,
};

#[derive(Clone, DataSize, Debug)]
pub(crate) struct Config {
    /// Maximum number of trie nodes to fetch in parallel.
    max_parallel_trie_fetches: u32,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(
        "The GlobalStateSynchronizer is already handling a request for {block_hash_and_height}"
    )]
    AlreadyHandlingARequest {
        block_hash_and_height: BlockHashAndHeight,
    },
    #[error("TrieAccumulator couldn't complete our request: {0}")]
    RequestFailed(TrieAccumulatorError),
    #[error("ContractRuntime failed to put a trie into global state: {0}")]
    PutTrie(engine_state::Error),
}

#[derive(Debug, Serialize)]
pub(crate) struct SyncGlobalState {
    block_hash_and_height: BlockHashAndHeight,
    state_root_hash: Digest,
    peers: HashSet<NodeId>,
    #[serde(skip)]
    responder: Responder<Result<(), Error>>,
}

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    Request(SyncGlobalState),
    FetchedTrie {
        trie_hash: Digest,
        trie_accumulator_result: TrieAccumulatorResult,
    },
    PutTrieResult {
        trie_hash: Digest,
        #[serde(skip)]
        put_trie_result: Result<Vec<Digest>, engine_state::Error>,
    },
}

struct RequestState {
    block_hash_and_height: BlockHashAndHeight,
    missing_descendants: HashSet<Digest>,
    in_flight: HashSet<Digest>,
    peers: HashSet<NodeId>,
    responder: Responder<Result<(), Error>>,
}

impl RequestState {
    fn new(request: SyncGlobalState) -> Self {
        let mut missing_descendants_for_current_block = HashSet::new();
        missing_descendants_for_current_block.insert(request.state_root_hash);
        Self {
            block_hash_and_height: request.block_hash_and_height,
            missing_descendants: missing_descendants_for_current_block,
            in_flight: HashSet::new(),
            peers: request.peers,
            responder: request.responder,
        }
    }

    fn add_missing_descendants(&mut self, missing_descendants: Vec<Digest>) {
        let descendants_to_add: Vec<_> = missing_descendants
            .into_iter()
            .filter(|descendant_hash| !self.in_flight.contains(descendant_hash))
            .collect();
        self.missing_descendants.extend(descendants_to_add);
    }
}

pub(crate) struct GlobalStateSynchronizer {
    max_parallel_trie_fetches: usize,
    current_request_state: Option<RequestState>,
}

impl GlobalStateSynchronizer {
    fn new(config: Config) -> Self {
        Self {
            max_parallel_trie_fetches: config.max_parallel_trie_fetches as usize,
            current_request_state: None,
        }
    }

    fn handle_request<REv>(
        &mut self,
        request: SyncGlobalState,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        match &mut self.current_request_state {
            None => {
                self.current_request_state = Some(RequestState::new(request));
            }
            Some(request_state)
                if request_state.block_hash_and_height != request.block_hash_and_height =>
            {
                debug!(
                    handling_block=%request_state.block_hash_and_height,
                    requested_block=%request.block_hash_and_height,
                    "already handling a request"
                );
                return request
                    .responder
                    .respond(Err(Error::AlreadyHandlingARequest {
                        block_hash_and_height: request_state.block_hash_and_height,
                    }))
                    .ignore();
            }
            Some(request_state) => {
                request_state.peers.extend(request.peers);
            }
        }

        self.parallel_fetch(effect_builder)
    }

    fn parallel_fetch<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        let request_state = match &mut self.current_request_state {
            Some(state) => state,
            None => {
                debug!("calling parallel_fetch with no current request");
                return Effects::new();
            }
        };

        // if there are no missing descendants and no tries in flight, we're finished
        if request_state.missing_descendants.is_empty() && request_state.in_flight.is_empty() {
            return self.finish_request();
        }

        // if we're not finished, figure out how many new fetching tasks we can start
        let num_fetches_to_start = self
            .max_parallel_trie_fetches
            .saturating_sub(request_state.in_flight.len());
        let mut requested_hashes = HashSet::new();
        let mut effects = Effects::new();

        for trie_hash in request_state
            .missing_descendants
            .iter()
            .filter(|trie_hash| !request_state.in_flight.contains(*trie_hash))
            .take(num_fetches_to_start)
            .cloned()
        {
            effects.extend(
                effect_builder
                    .fetch_trie(trie_hash, request_state.peers.iter().copied().collect())
                    .event(move |trie_accumulator_result| Event::FetchedTrie {
                        trie_hash,
                        trie_accumulator_result,
                    }),
            );
            requested_hashes.insert(trie_hash);
        }

        request_state.in_flight.extend(requested_hashes);
        request_state.missing_descendants = request_state
            .missing_descendants
            .difference(&request_state.in_flight)
            .copied()
            .collect();

        effects
    }

    fn handle_fetched_trie<REv>(
        &mut self,
        trie_hash: Digest,
        trie_accumulator_result: TrieAccumulatorResult,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let trie_raw = match trie_accumulator_result {
            Ok(trie_raw) => trie_raw,
            Err(error) => {
                debug!(%error, "error fetching a trie");
                return self.cancel_request(Error::RequestFailed(error));
            }
        };

        effect_builder
            .put_trie_and_find_missing_descendant_trie_keys(*trie_raw)
            .event(move |put_trie_result| Event::PutTrieResult {
                trie_hash,
                put_trie_result,
            })
    }

    fn cancel_request(&mut self, error: Error) -> Effects<Event> {
        match self.current_request_state.take() {
            Some(request_state) => request_state.responder.respond(Err(error)).ignore(),
            None => Effects::new(),
        }
    }

    fn finish_request(&mut self) -> Effects<Event> {
        match self.current_request_state.take() {
            Some(request_state) => request_state.responder.respond(Ok(())).ignore(),
            None => Effects::new(),
        }
    }

    fn handle_put_trie_result<REv>(
        &mut self,
        trie_hash: Digest,
        put_trie_result: Result<Vec<Digest>, engine_state::Error>,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        let request_state = match &mut self.current_request_state {
            Some(state) => state,
            None => {
                debug!(%trie_hash, "calling handle_put_trie_result with no current request");
                return Effects::new();
            }
        };

        match put_trie_result {
            Ok(missing_descendants) => {
                request_state.add_missing_descendants(missing_descendants);
                request_state.in_flight.remove(&trie_hash);
                self.parallel_fetch(effect_builder)
            }
            Err(error) => {
                warn!(%trie_hash, %error, "couldn't put trie into global state");
                self.cancel_request(Error::PutTrie(error))
            }
        }
    }
}

impl<REv> Component<REv> for GlobalStateSynchronizer
where
    REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(request) => self.handle_request(request, effect_builder),
            Event::FetchedTrie {
                trie_hash,
                trie_accumulator_result,
            } => self.handle_fetched_trie(trie_hash, trie_accumulator_result, effect_builder),
            Event::PutTrieResult {
                trie_hash,
                put_trie_result,
            } => self.handle_put_trie_result(trie_hash, put_trie_result, effect_builder),
        }
    }
}
