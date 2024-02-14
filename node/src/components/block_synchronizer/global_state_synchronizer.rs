#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashSet},
    fmt, mem,
};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_storage::{
    data_access_layer::{PutTrieRequest, PutTrieResult},
    global_state::{error::Error as GlobalStateError, trie::TrieRaw},
};
use casper_types::{BlockHash, Digest, DisplayIter, Timestamp};

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
    types::{NodeId, TrieOrChunk},
    NodeRng,
};

const COMPONENT_NAME: &str = "global_state_synchronizer";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DataSize, From)]
pub(crate) struct RootHash(Digest);

impl RootHash {
    #[cfg(test)]
    pub(crate) fn new(digest: Digest) -> Self {
        Self(digest)
    }

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
    #[error("Failed to persist trie element in global state: {0}; unreliable peers {}", DisplayIter::new(.1))]
    PutTrie(GlobalStateError, Vec<NodeId>),
    #[error("no peers available to ask for a trie")]
    NoPeersAvailable,
    #[error("received request for {hash_requested} while syncing another root hash: {hash_being_synced}")]
    ProcessingAnotherRequest {
        hash_being_synced: Digest,
        hash_requested: Digest,
    },
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
    GetPeers(Vec<NodeId>),
    FetchedTrie {
        trie_hash: TrieHash,
        trie_accumulator_result: Result<TrieAccumulatorResponse, TrieAccumulatorError>,
    },
    PutTrieResult {
        #[serde(skip)]
        raw: TrieRaw,
        #[serde(skip)]
        result: PutTrieResult,
    },
    #[from]
    TrieAccumulatorEvent(TrieAccumulatorEvent),
}

#[derive(Debug, DataSize)]
struct RequestState {
    root_hash: RootHash,
    block_hashes: HashSet<BlockHash>,
    responders: Vec<Responder<Result<Response, Error>>>,
    unreliable_peers: HashSet<NodeId>,
}

impl RequestState {
    fn new(request: SyncGlobalStateRequest) -> Self {
        let mut block_hashes = HashSet::new();
        block_hashes.insert(request.block_hash);
        Self {
            root_hash: RootHash(request.state_root_hash),
            block_hashes,
            responders: vec![request.responder],
            unreliable_peers: HashSet::new(),
        }
    }

    /// Extends the responders based on an additional request.
    fn add_request(&mut self, request: SyncGlobalStateRequest) {
        self.block_hashes.insert(request.block_hash);
        self.responders.push(request.responder);
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
}

impl TrieAwaitingChildren {
    fn new(trie_raw: TrieRaw, missing_children: Vec<TrieHash>) -> Self {
        Self {
            trie_raw,
            missing_children: missing_children.into_iter().collect(),
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

    fn into_trie_raw(self) -> TrieRaw {
        self.trie_raw
    }
}

#[derive(Debug, Default, DataSize)]
struct FetchQueue {
    queue: Vec<TrieHash>,
    /// set of the same values that are in the queue - so that we can quickly check that we do not
    /// duplicate the same entry in the queue
    hashes_set: HashSet<TrieHash>,
}

impl FetchQueue {
    fn insert(&mut self, trie_hash: TrieHash) {
        if self.hashes_set.insert(trie_hash) {
            self.queue.push(trie_hash);
        }
    }

    fn take(&mut self, num_to_take: usize) -> Vec<TrieHash> {
        // `to_return` will contain `num_to_take` elements from the end of the queue (or all of
        // them if `num_to_take` is greater than queue length).
        // Taking elements from the end will essentially make our traversal depth-first instead of
        // breadth-first.
        let to_return = self
            .queue
            .split_off(self.queue.len().saturating_sub(num_to_take));
        // remove the returned hashes from the "duplication prevention" set
        for returned_hash in &to_return {
            self.hashes_set.remove(returned_hash);
        }
        to_return
    }

    fn handle_request_cancelled(&mut self) {
        self.queue = vec![];
        self.hashes_set = HashSet::new();
    }
}

#[derive(Debug, DataSize)]
pub(super) struct GlobalStateSynchronizer {
    max_parallel_trie_fetches: usize,
    trie_accumulator: TrieAccumulator,
    request_state: Option<RequestState>,
    // TODO: write some smarter cache that purges stale entries and limits memory usage
    tries_awaiting_children: BTreeMap<TrieHash, TrieAwaitingChildren>,
    fetch_queue: FetchQueue,
    in_flight: HashSet<TrieHash>,
    last_progress: Option<Timestamp>,
}

impl GlobalStateSynchronizer {
    pub(super) fn new(max_parallel_trie_fetches: usize) -> Self {
        Self {
            max_parallel_trie_fetches,
            trie_accumulator: TrieAccumulator::new(),
            request_state: None,
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
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let state_root_hash = request.state_root_hash;

        let mut effects = match &mut self.request_state {
            None => {
                self.request_state = Some(RequestState::new(request));
                self.touch();
                self.enqueue_trie_for_fetching(effect_builder, TrieHash(state_root_hash))
            }
            Some(state) => {
                if state.root_hash.0 != state_root_hash {
                    return request
                        .responder
                        .respond(Err(Error::ProcessingAnotherRequest {
                            hash_being_synced: state.root_hash.0,
                            hash_requested: state_root_hash,
                        }))
                        .ignore();
                } else {
                    state.add_request(request);
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

    fn parallel_fetch<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event> {
        effect_builder
            .immediately()
            .event(|()| Event::GetPeers(vec![]))
    }

    fn parallel_fetch_with_peers<REv>(
        &mut self,
        peers: Vec<NodeId>,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + Send,
    {
        let mut effects = Effects::new();

        if self.request_state.is_none() {
            debug!("called parallel_fetch while not processing any requests");
            return effects;
        }

        // Just to not overdo parallel trie fetches in small networks. 5000 parallel trie fetches
        // seemed to be fine in networks of 100 peers, so we set the limit at 50 * number of peers.
        let max_parallel_trie_fetches = self.max_parallel_trie_fetches.min(peers.len() * 50);

        // if we're not finished, figure out how many new fetching tasks we can start
        let num_fetches_to_start = max_parallel_trie_fetches.saturating_sub(self.in_flight.len());

        debug!(
            max_parallel_trie_fetches,
            in_flight_length = self.in_flight.len(),
            fetch_queue_length = self.fetch_queue.queue.len(),
            num_fetches_to_start,
            "parallel_fetch"
        );

        let to_fetch = self.fetch_queue.take(num_fetches_to_start);

        if peers.is_empty() {
            // if we have no peers, fail - trie accumulator would return an error, anyway
            debug!("no peers available, cancelling request");
            return self.cancel_request(Error::NoPeersAvailable);
        }

        for trie_hash in to_fetch {
            if self.in_flight.insert(trie_hash) {
                effects.extend(effect_builder.fetch_trie(trie_hash.0, peers.clone()).event(
                    move |trie_accumulator_result| Event::FetchedTrie {
                        trie_hash,
                        trie_accumulator_result,
                    },
                ));
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
        // A result of `false` probably indicates that this is a stale fetch from a previously
        // cancelled request - we shouldn't cancel the current request if the result is an error in
        // such a case.
        let in_flight_was_present = self.in_flight.remove(&trie_hash);

        debug!(
            %trie_hash,
            in_flight_length = self.in_flight.len(),
            fetch_queue_length = self.fetch_queue.queue.len(),
            processing_request = self.request_state.is_some(),
            "handle_fetched_trie"
        );

        let trie_raw = match trie_accumulator_result {
            Ok(response) => {
                if let Some(request_state) = &mut self.request_state {
                    request_state
                        .unreliable_peers
                        .extend(response.unreliable_peers());
                }
                response.trie()
            }
            Err(error) => {
                debug!(%error, "error fetching a trie");
                let new_unreliable_peers = match error {
                    TrieAccumulatorError::Absent(_, _, unreliable_peers)
                    | TrieAccumulatorError::PeersExhausted(_, unreliable_peers) => unreliable_peers,
                    TrieAccumulatorError::NoPeers(_) => {
                        // Trie accumulator did not have any peers to download from
                        // so the request will be canceled with no peers to report
                        vec![]
                    }
                };
                let unreliable_peers = self.request_state.as_mut().map_or_else(Vec::new, |state| {
                    state.unreliable_peers.extend(new_unreliable_peers);
                    state.unreliable_peers.iter().copied().collect()
                });
                debug!(%trie_hash, "unreliable peers for requesting trie, cancelling request");
                let mut effects = if in_flight_was_present {
                    self.cancel_request(Error::TrieAccumulator(unreliable_peers))
                } else {
                    Effects::new()
                };

                // continue fetching other requests if any
                // request_state might be `None` if we are processing fetch responses that were in
                // flight when we cancelled a request
                if self.request_state.is_some() {
                    effects.extend(self.parallel_fetch(effect_builder));
                }
                return effects;
            }
        };

        self.touch();

        let request = PutTrieRequest::new((*trie_raw).clone());
        effect_builder
            .put_trie_if_all_children_present(request)
            .event(move |put_trie_result| Event::PutTrieResult {
                raw: *trie_raw,
                result: put_trie_result,
            })
    }

    pub(super) fn cancel_request(&mut self, error: Error) -> Effects<Event> {
        match self.request_state.take() {
            Some(request_state) => {
                debug!(root_hash=%request_state.root_hash, "cancelling request");
                self.fetch_queue.handle_request_cancelled();
                self.in_flight = HashSet::new();
                request_state.respond(Err(error))
            }
            None => {
                debug!("not cancelling request - none being processed");
                Effects::new()
            }
        }
    }

    fn finish_request(&mut self) -> Effects<Event> {
        match self.request_state.take() {
            Some(request_state) => {
                let root_hash = request_state.root_hash;
                debug!(%root_hash, "finishing request");
                let unreliable_peers = request_state.unreliable_peers.iter().copied().collect();
                request_state.respond(Ok(Response::new(root_hash, unreliable_peers)))
            }
            None => {
                // We only call this function after checking that we are processing a request - if
                // the request is None, this is a bug
                error!("not finishing request - none being processed");
                Effects::new()
            }
        }
    }

    fn handle_put_trie_result<REv>(
        &mut self,
        requested_hash: Digest,
        put_trie_result: PutTrieResult,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        let mut effects = Effects::new();

        match put_trie_result {
            PutTrieResult::Success { hash } if hash == requested_hash => {
                effects.extend(self.handle_trie_written(effect_builder, TrieHash(hash)))
            }
            PutTrieResult::Success { hash } => {
                error!(
                    %hash,
                    %requested_hash,
                    "trie was stored under a different hash than was used to request it - \
                    it's a bug"
                );
            }
            PutTrieResult::Failure(GlobalStateError::MissingTrieNodeChildren(
                trie_hash,
                trie_raw,
                missing_children,
            )) => effects.extend(self.handle_trie_missing_children(
                effect_builder,
                TrieHash(trie_hash),
                trie_raw,
                missing_children.into_iter().map(TrieHash).collect(),
            )),
            PutTrieResult::Failure(gse) => {
                warn!(%requested_hash, %gse, "couldn't put trie into global state");
                if let Some(request_state) = &mut self.request_state {
                    let unreliable_peers = request_state.unreliable_peers.iter().copied().collect();
                    effects.extend(self.cancel_request(Error::PutTrie(gse, unreliable_peers)));
                }
            }
        }

        // request_state can be none if we're processing a result of a fetch that was in flight
        // when a request got cancelled
        if self.request_state.is_some() {
            effects.extend(self.parallel_fetch(effect_builder));
        }

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
        self.touch();

        // Remove the written trie from dependencies of the tries that are waiting.
        for trie_awaiting in self.tries_awaiting_children.values_mut() {
            trie_awaiting.trie_written(written_trie);
        }

        let (ready_tries, still_incomplete): (BTreeMap<_, _>, BTreeMap<_, _>) =
            mem::take(&mut self.tries_awaiting_children)
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
            .flat_map(|(_, trie_awaiting)| {
                let trie_raw = trie_awaiting.into_trie_raw();
                let request = PutTrieRequest::new(trie_raw.clone());
                effect_builder
                    .put_trie_if_all_children_present(request)
                    .event(move |result| Event::PutTrieResult {
                        raw: trie_raw,
                        result,
                    })
            })
            .collect();

        // If there is a request state associated with the trie we just wrote, it means that it was
        // a root trie and we can report fetching to be finished.
        if let Some(request_state) = &mut self.request_state {
            if TrieHash(request_state.root_hash.0) == written_trie {
                effects.extend(self.finish_request());
            }
        }

        effects
    }

    fn enqueue_trie_for_fetching<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trie_hash: TrieHash,
    ) -> Effects<Event>
    where
        REv: From<ContractRuntimeRequest> + Send,
    {
        // we might have fetched it already!
        if let Some(trie_awaiting) = self.tries_awaiting_children.get_mut(&trie_hash) {
            // simulate fetching having been completed in order to start fetching any children that
            // might be still missing
            let trie_raw = trie_awaiting.trie_raw.clone();
            let request = PutTrieRequest::new(trie_raw.clone());
            effect_builder
                .put_trie_if_all_children_present(request)
                .event(move |result| Event::PutTrieResult {
                    raw: trie_raw,
                    result,
                })
        } else {
            // otherwise, add to the queue
            self.fetch_queue.insert(trie_hash);
            Effects::new()
        }
    }

    fn handle_trie_missing_children<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trie_hash: TrieHash,
        trie_raw: TrieRaw,
        missing_children: Vec<TrieHash>,
    ) -> Effects<Event>
    where
        REv: From<TrieAccumulatorRequest> + From<ContractRuntimeRequest> + Send,
    {
        if self.request_state.is_none() {
            // this can be valid if we're processing a fetch result that was in flight while we
            // were cancelling a request - but we don't want to continue queueing further tries for
            // fetching
            return Effects::new();
        }

        self.touch();

        let mut effects: Effects<Event> = missing_children
            .iter()
            .flat_map(|child| self.enqueue_trie_for_fetching(effect_builder, *child))
            .collect();
        self.tries_awaiting_children.insert(
            trie_hash,
            TrieAwaitingChildren::new(trie_raw, missing_children),
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
            Event::GetPeers(peers) => self.parallel_fetch_with_peers(peers, effect_builder),
            Event::FetchedTrie {
                trie_hash,
                trie_accumulator_result,
            } => self.handle_fetched_trie(trie_hash, trie_accumulator_result, effect_builder),
            Event::PutTrieResult {
                raw: trie_raw,
                result: put_trie_result,
            } => self.handle_put_trie_result(trie_raw.hash(), put_trie_result, effect_builder),
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
