#[cfg(test)]
mod tests;

use std::{
    collections::{btree_map::Entry, BTreeMap, HashSet},
    fmt, mem,
};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_execution_engine::{core::engine_state, storage::trie::TrieRaw};
use casper_hashing::Digest;
use casper_types::Timestamp;

use super::{TrieAccumulator, TrieAccumulatorError, TrieAccumulatorEvent, TrieAccumulatorResponse};
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
    utils::DisplayIter,
    NodeRng,
};

const COMPONENT_NAME: &str = "global_state_synchronizer";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DataSize, From)]
pub(crate) struct RootHash(Digest);

impl RootHash {
    pub(crate) fn into_inner(self) -> Digest {
        self.0
    }
}

impl fmt::Display for RootHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DataSize, From)]
pub(crate) struct TrieHash(Digest);

impl fmt::Display for TrieHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Error)]
pub(crate) enum Error {
    #[error("trie accumulator encountered an error while fetching a trie; unreliable peers {}", DisplayIter::new(.0))]
    TrieAccumulator(Vec<NodeId>),
    #[error("ContractRuntime failed to put a trie into global state: {0}; unreliable peers {}", DisplayIter::new(.1))]
    PutTrie(engine_state::Error, Vec<NodeId>),
    #[error("no peers available to ask for a trie: {0}")]
    NoPeersAvailable(TrieHash),
}

#[derive(Debug, Clone)]
pub(crate) struct Response {
    hash: RootHash,
    unreliable_peers: Vec<NodeId>,
}

impl Response {
    pub(crate) fn new(hash: RootHash, unreliable_peers: Vec<NodeId>) -> Self {
        Self {
            hash,
            unreliable_peers,
        }
    }

    pub(crate) fn hash(&self) -> &RootHash {
        &self.hash
    }

    pub(crate) fn unreliable_peers(self) -> Vec<NodeId> {
        self.unreliable_peers
    }
}

#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    Request(SyncGlobalStateRequest),
    FetchedTrie {
        trie_hash: TrieHash,
        trie_accumulator_result: Result<TrieAccumulatorResponse, TrieAccumulatorError>,
    },
    PutTrieResult {
        trie_hash: TrieHash,
        trie_raw: TrieRaw,
        request_root_hashes: HashSet<RootHash>,
        #[serde(skip)]
        put_trie_result: Result<TrieHash, engine_state::Error>,
    },
    #[from]
    TrieAccumulatorEvent(TrieAccumulatorEvent),
}

#[derive(Debug, DataSize)]
struct RequestState {
    block_hashes: HashSet<BlockHash>,
    peers: HashSet<NodeId>,
    responders: Vec<Responder<Result<Response, Error>>>,
    unreliable_peers: HashSet<NodeId>,
}

impl RequestState {
    fn new(request: SyncGlobalStateRequest) -> Self {
        let mut block_hashes = HashSet::new();
        block_hashes.insert(request.block_hash);
        Self {
            block_hashes,
            peers: request.peers,
            responders: vec![request.responder],
            unreliable_peers: HashSet::new(),
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
    fn respond(self, response: Result<Response, Error>) -> Effects<Event> {
        self.responders
            .into_iter()
            .flat_map(|responder| responder.respond(response.clone()).ignore())
            .collect()
    }
}

#[derive(Debug, DataSize)]
struct TrieAwaitingChildren {
    trie_raw: TrieRaw,
    missing_children: HashSet<TrieHash>,
    request_root_hashes: HashSet<RootHash>,
}

impl TrieAwaitingChildren {
    fn new(
        trie_raw: TrieRaw,
        missing_children: Vec<TrieHash>,
        request_root_hashes: HashSet<RootHash>,
    ) -> Self {
        Self {
            trie_raw,
            missing_children: missing_children.into_iter().collect(),
            request_root_hashes,
        }
    }

    /// Handles `written_trie` being written to the database - removes the trie as a dependency and
    /// returns the next trie to be downloaded.
    fn trie_written(&mut self, written_trie: TrieHash) {
        self.missing_children.remove(&written_trie);
    }

    fn ready_to_be_written(&self) -> bool {
        self.missing_children.is_empty()
    }

    fn decompose(self) -> (TrieRaw, HashSet<RootHash>) {
        (self.trie_raw, self.request_root_hashes)
    }

    fn extend_request_hashes(&mut self, request_root_hashes: HashSet<RootHash>) {
        self.request_root_hashes.extend(request_root_hashes);
    }
}

#[derive(Debug, Default, DataSize)]
struct FetchQueue {
    queue: Vec<TrieHash>,
    /// a map of trie_hash → set of state root hashes awaiting this trie
    request_root_hashes: BTreeMap<TrieHash, HashSet<RootHash>>,
}

impl FetchQueue {
    fn insert(&mut self, trie_hash: TrieHash, request_root_hashes: HashSet<RootHash>) {
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

    fn take(&mut self, num_to_take: usize) -> Vec<(TrieHash, HashSet<RootHash>)> {
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

    fn handle_request_cancelled(&mut self, root_hash: RootHash) {
        self.request_root_hashes.retain(|_trie_hash, root_hashes| {
            root_hashes.remove(&root_hash);
            !root_hashes.is_empty()
        });
        // take the field temporarily out to avoid borrowing `self` in the closure later
        let request_root_hashes = mem::take(&mut self.request_root_hashes);
        // drop any queue entries that aren't related to any request anymore
        self.queue
            .retain(|trie_hash| request_root_hashes.contains_key(trie_hash));
        // closure is done, we can store the value back in `self`
        self.request_root_hashes = request_root_hashes;
    }
}

#[derive(Debug, DataSize)]
pub(super) struct GlobalStateSynchronizer {
    max_parallel_trie_fetches: usize,
    trie_accumulator: TrieAccumulator,
    request_states: BTreeMap<RootHash, RequestState>,
    tries_awaiting_children: BTreeMap<TrieHash, TrieAwaitingChildren>,
    fetch_queue: FetchQueue,
    /// A map with trie hashes as the keys, and the values being sets of state root hashes that
    /// were requested for syncing and have those tries as descendants.
    in_flight: BTreeMap<TrieHash, HashSet<RootHash>>,
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

    /// Returns whether we are already processing a request for the given hash.
    pub(super) fn has_global_state_request(&self, global_state_hash: &Digest) -> bool {
        self.request_states
            .contains_key(&RootHash(*global_state_hash))
    }

    fn handle_request<REv>(
        &mut self,
        request: SyncGlobalStateRequest,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let state_root_hash = request.state_root_hash;

        let mut effects = match self.request_states.entry(RootHash(state_root_hash)) {
            Entry::Vacant(entry) => {
                let mut root_hashes = HashSet::new();
                root_hashes.insert(RootHash(state_root_hash));
                entry.insert(RequestState::new(request));
                self.touch();
                self.enqueue_trie_for_fetching(
                    effect_builder,
                    TrieHash(state_root_hash),
                    root_hashes,
                )
            }
            Entry::Occupied(entry) => {
                if entry.into_mut().add_request(request) {
                    self.touch();
                }
                Effects::new()
            }
        };

        debug!(
            %state_root_hash,
            fetch_queue_length = self.fetch_queue.queue.len(),
            tries_awaiting_children_length = self.tries_awaiting_children.len(),
            "handle_request"
        );

        effects.extend(self.parallel_fetch(effect_builder));

        effects
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

        debug!(
            in_flight_length = self.in_flight.len(),
            fetch_queue_length = self.fetch_queue.queue.len(),
            request_states_length = self.request_states.len(),
            num_fetches_to_start,
            "parallel_fetch"
        );

        let to_fetch = self.fetch_queue.take(num_fetches_to_start);

        for (trie_hash, request_root_hashes) in to_fetch {
            let peers: Vec<_> = request_root_hashes
                .iter()
                .filter_map(|request_root_hash| self.request_states.get(request_root_hash))
                .flat_map(|request_state| request_state.peers.iter().copied())
                .collect();
            if peers.is_empty() {
                // if we have no peers, fail - trie accumulator would return an error, anyway
                debug!(%trie_hash, "no peers available for requesting trie, cancelling request");
                return request_root_hashes
                    .into_iter()
                    .flat_map(|root_hash| {
                        self.cancel_request(root_hash, Error::NoPeersAvailable(trie_hash))
                    })
                    .collect();
            } else {
                match self.in_flight.entry(trie_hash) {
                    Entry::Vacant(entry) => {
                        entry.insert(request_root_hashes);
                        effects.extend(effect_builder.fetch_trie(trie_hash.0, peers).event(
                            move |trie_accumulator_result| Event::FetchedTrie {
                                trie_hash,
                                trie_accumulator_result,
                            },
                        ));
                    }
                    Entry::Occupied(entry) => {
                        // we have already started fetching the trie - don't reattempt, only store
                        // the hashes of relevant state roots
                        entry.into_mut().extend(request_root_hashes);
                    }
                }
            }
        }

        effects
    }

    fn handle_fetched_trie<REv>(
        &mut self,
        trie_hash: TrieHash,
        trie_accumulator_result: Result<TrieAccumulatorResponse, TrieAccumulatorError>,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let request_root_hashes = if let Some(hashes) = self.in_flight.remove(&trie_hash) {
            hashes
        } else {
            error!(
                %trie_hash,
                "received FetchedTrie, but no in_flight entry is present - this is a bug"
            );
            HashSet::new()
        };

        debug!(
            in_flight_length = self.in_flight.len(),
            fetch_queue_length = self.fetch_queue.queue.len(),
            request_states_length = self.request_states.len(),
            request_root_hashes_length = request_root_hashes.len(),
            "handle_fetched_trie"
        );

        let trie_raw = match trie_accumulator_result {
            Ok(response) => {
                for root_hash in request_root_hashes.iter() {
                    if let Some(request_state) = self.request_states.get_mut(root_hash) {
                        request_state
                            .unreliable_peers
                            .extend(response.unreliable_peers());
                    }
                }
                response.trie()
            }
            Err(error) => {
                debug!(%error, "error fetching a trie");
                let mut effects = Effects::new();
                effects.extend(request_root_hashes.into_iter().flat_map(|root_hash| {
                    if let Some(request_state) = self.request_states.get_mut(&root_hash) {
                        match &error {
                            TrieAccumulatorError::Absent(_, _, unreliable_peers)
                            | TrieAccumulatorError::PeersExhausted(_, unreliable_peers) => {
                                request_state.unreliable_peers.extend(unreliable_peers);
                            }
                            TrieAccumulatorError::NoPeers(_) => {
                                // Trie accumulator did not have any peers to download from
                                // so the request will be canceled with no peers to report
                            }
                        }
                        let unreliable_peers =
                            request_state.unreliable_peers.iter().copied().collect();
                            debug!(%trie_hash, "unreliable peers for requesting trie, cancelling request");
                        self.cancel_request(root_hash, Error::TrieAccumulator(unreliable_peers))
                    } else {
                        Effects::new()
                    }
                }));
                // continue fetching other requests if any
                effects.extend(self.parallel_fetch(effect_builder));
                return effects;
            }
        };

        self.touch();

        effect_builder
            .put_trie_if_all_children_present((*trie_raw).clone())
            .event(move |put_trie_result| Event::PutTrieResult {
                trie_hash,
                trie_raw: *trie_raw,
                request_root_hashes,
                put_trie_result: put_trie_result.map(TrieHash),
            })
    }

    pub(super) fn cancel_request(&mut self, root_hash: RootHash, error: Error) -> Effects<Event> {
        match self.request_states.remove(&root_hash) {
            Some(request_state) => {
                debug!(%root_hash, "cancelling request");
                self.fetch_queue.handle_request_cancelled(root_hash);
                request_state.respond(Err(error))
            }
            None => {
                debug!(%root_hash, "not cancelling request - root hash not found");
                Effects::new()
            }
        }
    }

    fn finish_request(&mut self, trie_hash: TrieHash) -> Effects<Event> {
        let root_hash = RootHash(trie_hash.0); // we don't know if it is a root hash - but if it's
                                               // not, we just won't have a corresponding entry
        match self.request_states.remove(&root_hash) {
            Some(request_state) => {
                debug!(%root_hash, "finishing request");
                let unreliable_peers = request_state.unreliable_peers.iter().copied().collect();
                request_state.respond(Ok(Response::new(root_hash, unreliable_peers)))
            }
            None => {
                debug!(%root_hash, "not finishing request - root hash not found");
                Effects::new()
            }
        }
    }

    fn handle_put_trie_result<REv>(
        &mut self,
        trie_hash: TrieHash,
        trie_raw: TrieRaw,
        request_root_hashes: HashSet<RootHash>,
        put_trie_result: Result<TrieHash, engine_state::Error>,
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
                    missing_children.into_iter().map(TrieHash).collect(),
                ))
            }
            Err(error) => {
                warn!(%trie_hash, %error, "couldn't put trie into global state");
                for root_hash in request_root_hashes {
                    if let Some(request_state) = self.request_states.get_mut(&root_hash) {
                        let unreliable_peers =
                            request_state.unreliable_peers.iter().copied().collect();
                        effects.extend(self.cancel_request(
                            root_hash,
                            Error::PutTrie(error.clone(), unreliable_peers),
                        ));
                    }
                }
            }
        }

        effects.extend(self.parallel_fetch(effect_builder));
        effects
    }

    fn handle_trie_written<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        written_trie: TrieHash,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        // Remove the written trie from dependencies of the tries that are waiting.
        for trie_awaiting in self.tries_awaiting_children.values_mut() {
            trie_awaiting.trie_written(written_trie);
        }

        let (ready_tries, still_incomplete): (BTreeMap<_, _>, BTreeMap<_, _>) =
            std::mem::take(&mut self.tries_awaiting_children)
                .into_iter()
                .partition(|(_, trie_awaiting)| trie_awaiting.ready_to_be_written());
        debug!(
            ready_tries = ready_tries.len(),
            still_incomplete = still_incomplete.len(),
            "handle_trie_written"
        );
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
                        put_trie_result: put_trie_result.map(TrieHash),
                    })
            })
            .collect();

        // If there is a request state associated with the trie we just wrote, it means that it was
        // a root trie and we can report fetching to be finished.
        effects.extend(self.finish_request(written_trie));

        effects
    }

    fn enqueue_trie_for_fetching<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trie_hash: TrieHash,
        request_root_hashes: HashSet<RootHash>,
    ) -> Effects<Event>
    where
        REv: From<ContractRuntimeRequest> + Send,
    {
        // we might have fetched it already!
        if let Some(trie_awaiting) = self.tries_awaiting_children.get_mut(&trie_hash) {
            // if that's the case, just store the relevant requested hashes
            trie_awaiting.extend_request_hashes(request_root_hashes.clone());
            // simulate fetching having been completed in order to start fetching any children that
            // might be still missing
            let trie_raw = trie_awaiting.trie_raw.clone();
            effect_builder
                .put_trie_if_all_children_present(trie_raw.clone())
                .event(move |put_trie_result| Event::PutTrieResult {
                    trie_hash,
                    trie_raw,
                    request_root_hashes,
                    put_trie_result: put_trie_result.map(TrieHash),
                })
        } else {
            // otherwise, add to the queue
            self.fetch_queue.insert(trie_hash, request_root_hashes);
            Effects::new()
        }
    }

    fn handle_trie_missing_children<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trie_hash: TrieHash,
        request_root_hashes: HashSet<RootHash>,
        trie_raw: TrieRaw,
        missing_children: Vec<TrieHash>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let mut effects: Effects<Event> = missing_children
            .iter()
            .flat_map(|child| {
                self.enqueue_trie_for_fetching(effect_builder, *child, request_root_hashes.clone())
            })
            .collect();
        self.tries_awaiting_children.insert(
            trie_hash,
            TrieAwaitingChildren::new(trie_raw, missing_children, request_root_hashes),
        );
        effects.extend(self.parallel_fetch(effect_builder));
        effects
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
                trie_accumulator_result,
            } => self.handle_fetched_trie(trie_hash, trie_accumulator_result, effect_builder),
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

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}
