use std::collections::{btree_map::Entry, BTreeMap, HashSet};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_execution_engine::{core::engine_state, storage::trie::TrieRaw};
use casper_hashing::Digest;
use casper_types::Timestamp;

use super::{TrieAccumulator, TrieAccumulatorError, TrieAccumulatorEvent};
use crate::{
    components::Component,
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{
            ContractRuntimeRequest, FetcherRequest, SyncGlobalStateRequest, TrieAccumulatorRequest,
        },
        EffectBuilder, EffectExt, Effects, Responder,
    },
    reactor,
    types::{BlockHash, NodeId, TrieOrChunk},
    NodeRng,
};

#[derive(Debug, Clone, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    TrieAccumulator(TrieAccumulatorError),
    #[error("ContractRuntime failed to put a trie into global state: {0}")]
    PutTrie(engine_state::Error),
    #[error("request to fetch cancelled")]
    Cancelled,
}

#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    Request(SyncGlobalStateRequest),
    FetchedTrie {
        trie_hash: Digest,
        request_root_hashes: HashSet<Digest>,
        trie_accumulator_result: Result<Box<TrieRaw>, TrieAccumulatorError>,
    },
    PutTrieResult {
        trie_hash: Digest,
        trie_raw: TrieRaw,
        request_root_hashes: HashSet<Digest>,
        #[serde(skip)]
        put_trie_result: Result<Digest, engine_state::Error>,
    },
    #[from]
    TrieAccumulatorEvent(TrieAccumulatorEvent),
}

#[derive(Debug, DataSize)]
struct RequestState {
    block_hashes: HashSet<BlockHash>,
    peers: HashSet<NodeId>,
    responders: Vec<Responder<Result<Digest, Error>>>,
}

impl RequestState {
    fn new(request: SyncGlobalStateRequest) -> Self {
        let mut block_hashes = HashSet::new();
        block_hashes.insert(request.block_hash);
        Self {
            block_hashes,
            peers: request.peers,
            responders: vec![request.responder],
        }
    }

    /// Extends the responders and known peers based on an additional request.
    /// Returns `true` if we added some new peers to the peers list.
    fn add_request(&mut self, request: SyncGlobalStateRequest) -> bool {
        let old_peers_len = self.peers.len();
        self.peers.extend(request.peers);
        self.block_hashes.insert(request.block_hash);
        self.responders.push(request.responder);
        old_peers_len != self.peers.len()
    }

    /// Consumes this request state and sends the response on all responders.
    fn respond(self, response: Result<Digest, Error>) -> Effects<Event> {
        self.responders
            .into_iter()
            .flat_map(|responder| responder.respond(response.clone()).ignore())
            .collect()
    }
}

#[derive(Debug, DataSize)]
struct TrieAwaitingChildren {
    trie_raw: TrieRaw,
    missing_children: HashSet<Digest>,
    request_root_hashes: HashSet<Digest>,
}

impl TrieAwaitingChildren {
    fn new(
        trie_raw: TrieRaw,
        missing_children: Vec<Digest>,
        request_root_hashes: HashSet<Digest>,
    ) -> Self {
        Self {
            trie_raw,
            missing_children: missing_children.into_iter().collect(),
            request_root_hashes,
        }
    }

    /// Handles `written_trie` being written to the database - removes the trie as a dependency and
    /// returns the next trie to be downloaded.
    fn trie_written(&mut self, written_trie: Digest) {
        self.missing_children.remove(&written_trie);
    }

    fn ready_to_be_written(&self) -> bool {
        self.missing_children.is_empty()
    }

    fn decompose(self) -> (TrieRaw, HashSet<Digest>) {
        (self.trie_raw, self.request_root_hashes)
    }

    fn extend_request_hashes(&mut self, request_root_hashes: HashSet<Digest>) {
        self.request_root_hashes.extend(request_root_hashes);
    }
}

#[derive(Debug, Default, DataSize)]
struct FetchQueue {
    queue: Vec<Digest>,
    /// a map of trie_hash → set of state root hashes awaiting this trie
    request_root_hashes: BTreeMap<Digest, HashSet<Digest>>,
}

impl FetchQueue {
    fn insert(&mut self, trie_hash: Digest, request_root_hashes: HashSet<Digest>) {
        match self.request_root_hashes.entry(trie_hash) {
            Entry::Vacant(entry) => {
                entry.insert(request_root_hashes);
                self.queue.push(trie_hash);
            }
            Entry::Occupied(entry) => {
                entry.into_mut().extend(request_root_hashes);
            }
        }
    }

    fn take(&mut self, num_to_take: usize) -> Vec<(Digest, HashSet<Digest>)> {
        // `to_return` will contain `num_to_take` elements from the end of the queue (or all of
        // them if `num_to_take` is greater than queue length).
        // Taking elements from the end will essentially make our traversal depth-first instead of
        // breadth-first.
        let to_return = self
            .queue
            .split_off(self.queue.len().saturating_sub(num_to_take));
        to_return
            .into_iter()
            .filter_map(|trie_hash| {
                self.request_root_hashes
                    .remove(&trie_hash)
                    .map(|root_set| (trie_hash, root_set))
            })
            .collect()
    }
}

#[derive(Debug, DataSize)]
pub(super) struct GlobalStateSynchronizer {
    max_parallel_trie_fetches: usize,
    trie_accumulator: TrieAccumulator,
    request_states: BTreeMap<Digest, RequestState>,
    tries_awaiting_children: BTreeMap<Digest, TrieAwaitingChildren>,
    fetch_queue: FetchQueue,
    in_flight: HashSet<Digest>,
    last_progress: Option<Timestamp>,
}

impl GlobalStateSynchronizer {
    pub(super) fn new(max_parallel_trie_fetches: usize) -> Self {
        Self {
            max_parallel_trie_fetches,
            trie_accumulator: TrieAccumulator::new(),
            request_states: Default::default(),
            tries_awaiting_children: Default::default(),
            fetch_queue: Default::default(),
            in_flight: Default::default(),
            last_progress: None,
        }
    }

    fn touch(&mut self) {
        self.last_progress = Some(Timestamp::now());
    }

    pub(super) fn last_progress(&self) -> Option<Timestamp> {
        self.last_progress
    }

    fn handle_request<REv>(
        &mut self,
        request: SyncGlobalStateRequest,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        let state_root_hash = request.state_root_hash;
        match self.request_states.entry(state_root_hash) {
            Entry::Vacant(entry) => {
                let mut root_hashes = HashSet::new();
                root_hashes.insert(state_root_hash);
                entry.insert(RequestState::new(request));
                self.enqueue_trie_for_fetching(state_root_hash, root_hashes);
                self.touch();
            }
            Entry::Occupied(entry) => {
                if entry.into_mut().add_request(request) {
                    self.touch();
                }
            }
        }

        self.parallel_fetch(effect_builder)
    }

    fn parallel_fetch<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        let mut effects = Effects::new();

        // if we're not finished, figure out how many new fetching tasks we can start
        let num_fetches_to_start = self
            .max_parallel_trie_fetches
            .saturating_sub(self.in_flight.len());

        let to_fetch = self.fetch_queue.take(num_fetches_to_start);

        for (trie_hash, request_root_hashes) in to_fetch {
            let peers: Vec<_> = request_root_hashes
                .iter()
                .filter_map(|request_root_hash| self.request_states.get(request_root_hash))
                .flat_map(|request_state| request_state.peers.iter().copied())
                .collect();
            if peers.is_empty() {
                // if we have no peers, requeue the trie - trie accumulator would return an error,
                // anyway
                debug!(%trie_hash, "no peers available for requesting trie; requeuing");
                self.enqueue_trie_for_fetching(trie_hash, request_root_hashes);
            } else {
                effects.extend(effect_builder.fetch_trie(trie_hash, peers).event(
                    move |trie_accumulator_result| Event::FetchedTrie {
                        trie_hash,
                        request_root_hashes,
                        trie_accumulator_result,
                    },
                ));
                self.in_flight.insert(trie_hash);
            }
        }

        effects
    }

    fn handle_fetched_trie<REv>(
        &mut self,
        trie_hash: Digest,
        request_root_hashes: HashSet<Digest>,
        trie_accumulator_result: Result<Box<TrieRaw>, TrieAccumulatorError>,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let trie_raw = match trie_accumulator_result {
            Ok(trie_raw) => trie_raw,
            Err(error) => {
                debug!(%error, "error fetching a trie");
                return request_root_hashes
                    .into_iter()
                    .flat_map(|root_hash| {
                        self.cancel_request(root_hash, Error::TrieAccumulator(error.clone()))
                    })
                    .collect();
            }
        };

        self.touch();

        self.in_flight.remove(&trie_hash);
        effect_builder
            .put_trie_if_all_children_present((*trie_raw).clone())
            .event(move |put_trie_result| Event::PutTrieResult {
                trie_hash,
                trie_raw: *trie_raw,
                request_root_hashes,
                put_trie_result,
            })
    }

    pub(super) fn cancel_request(&mut self, root_hash: Digest, error: Error) -> Effects<Event> {
        match self.request_states.remove(&root_hash) {
            Some(request_state) => request_state.respond(Err(error)),
            None => Effects::new(),
        }
    }

    fn finish_request(&mut self, trie_hash: Digest) -> Effects<Event> {
        match self.request_states.remove(&trie_hash) {
            Some(request_state) => request_state.respond(Ok(trie_hash)),
            None => Effects::new(),
        }
    }

    fn handle_put_trie_result<REv>(
        &mut self,
        trie_hash: Digest,
        trie_raw: TrieRaw,
        request_root_hashes: HashSet<Digest>,
        put_trie_result: Result<Digest, engine_state::Error>,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let mut effects = Effects::new();

        match put_trie_result {
            Ok(digest) if digest == trie_hash => {
                effects.extend(self.handle_trie_written(effect_builder, digest))
            }
            Ok(digest) => {
                error!(
                    %digest,
                    %trie_hash,
                    "trie was stored under a different hash than was used to request it - \
                    it's a bug"
                );
            }
            Err(engine_state::Error::MissingTrieNodeChildren(missing_children)) => {
                effects.extend(self.handle_trie_missing_children(
                    effect_builder,
                    trie_hash,
                    request_root_hashes,
                    trie_raw,
                    missing_children,
                ))
            }
            Err(error) => {
                warn!(%trie_hash, %error, "couldn't put trie into global state");
                for root_hash in request_root_hashes {
                    effects.extend(self.cancel_request(root_hash, Error::PutTrie(error.clone())));
                }
            }
        }

        effects.extend(self.parallel_fetch(effect_builder));
        effects
    }

    fn handle_trie_written<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        written_trie: Digest,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        // Remove the written trie from dependencies of the tries that are waiting.
        for trie_awaiting in self.tries_awaiting_children.values_mut() {
            trie_awaiting.trie_written(written_trie);
        }

        let (ready_tries, still_incomplete) = std::mem::take(&mut self.tries_awaiting_children)
            .into_iter()
            .partition(|(_, trie_awaiting)| trie_awaiting.ready_to_be_written());
        self.tries_awaiting_children = still_incomplete;

        let mut effects: Effects<Event> = ready_tries
            .into_iter()
            .flat_map(|(trie_hash, trie_awaiting)| {
                let (trie_raw, request_root_hashes) = trie_awaiting.decompose();
                effect_builder
                    .put_trie_if_all_children_present(trie_raw.clone())
                    .event(move |put_trie_result| Event::PutTrieResult {
                        trie_hash,
                        trie_raw,
                        request_root_hashes,
                        put_trie_result,
                    })
            })
            .collect();

        // If there is a request state associated with the trie we just wrote, it means that it was
        // a root trie and we can report fetching to be finished.
        effects.extend(self.finish_request(written_trie));

        effects
    }

    fn enqueue_trie_for_fetching(
        &mut self,
        trie_hash: Digest,
        request_root_hashes: HashSet<Digest>,
    ) {
        // we might have fetched it already!
        if let Some(trie_awaiting) = self.tries_awaiting_children.get_mut(&trie_hash) {
            // if that's the case, just store the relevant requested hashes
            trie_awaiting.extend_request_hashes(request_root_hashes);
        } else {
            // otherwise, add to the queue
            self.fetch_queue.insert(trie_hash, request_root_hashes);
        }
    }

    fn handle_trie_missing_children<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trie_hash: Digest,
        request_root_hashes: HashSet<Digest>,
        trie_raw: TrieRaw,
        missing_children: Vec<Digest>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        for child in &missing_children {
            self.enqueue_trie_for_fetching(*child, request_root_hashes.clone());
        }
        self.tries_awaiting_children.insert(
            trie_hash,
            TrieAwaitingChildren::new(trie_raw, missing_children, request_root_hashes),
        );
        self.parallel_fetch(effect_builder)
    }
}

impl<REv> Component<REv> for GlobalStateSynchronizer
where
    REv: From<TrieAccumulatorRequest>
        + From<ContractRuntimeRequest>
        + From<FetcherRequest<TrieOrChunk>>
        + From<PeerBehaviorAnnouncement>
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
            Event::Request(request) => self.handle_request(request, effect_builder),
            Event::FetchedTrie {
                trie_hash,
                request_root_hashes,
                trie_accumulator_result,
            } => self.handle_fetched_trie(
                trie_hash,
                request_root_hashes,
                trie_accumulator_result,
                effect_builder,
            ),
            Event::PutTrieResult {
                trie_hash,
                trie_raw,
                request_root_hashes,
                put_trie_result,
            } => self.handle_put_trie_result(
                trie_hash,
                trie_raw,
                request_root_hashes,
                put_trie_result,
                effect_builder,
            ),
            Event::TrieAccumulatorEvent(event) => reactor::wrap_effects(
                Event::TrieAccumulatorEvent,
                self.trie_accumulator
                    .handle_event(effect_builder, rng, event),
            ),
        }
    }
}
