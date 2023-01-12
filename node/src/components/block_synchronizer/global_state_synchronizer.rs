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

const COMPONENT_NAME: &str = "global_state_synchronizer";

#[derive(Debug, Clone, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    TrieAccumulator(TrieAccumulatorError),
    #[error("ContractRuntime failed to put a trie into global state: {0}")]
    PutTrie(engine_state::Error),
    #[error("no peers available to ask for a trie: {0}")]
    NoPeersAvailable(Digest),
}

#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    Request(SyncGlobalStateRequest),
    FetchedTrie {
        trie_hash: Digest,
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
    /// A map with trie hashes as the keys, and the values being sets of state root hashes that
    /// were requested for syncing and have those tries as descendants.
    in_flight: BTreeMap<Digest, HashSet<Digest>>,
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
                // if we have no peers, fail - trie accumulator would return an error, anyway
                debug!(%trie_hash, "no peers available for requesting trie");
                return self.cancel_request(trie_hash, Error::NoPeersAvailable(trie_hash));
            } else {
                match self.in_flight.entry(trie_hash) {
                    Entry::Vacant(entry) => {
                        entry.insert(request_root_hashes);
                        effects.extend(effect_builder.fetch_trie(trie_hash, peers).event(
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
        trie_hash: Digest,
        trie_accumulator_result: Result<Box<TrieRaw>, TrieAccumulatorError>,
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

        let trie_raw = match trie_accumulator_result {
            Ok(trie_raw) => trie_raw,
            Err(error) => {
                debug!(%error, "error fetching a trie");
                let mut effects = Effects::new();
                effects.extend(request_root_hashes.into_iter().flat_map(|root_hash| {
                    self.cancel_request(root_hash, Error::TrieAccumulator(error.clone()))
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        reactor::{EventQueueHandle, QueueKind, Scheduler},
        types::Block,
        utils,
    };
    use casper_types::{bytesrepr::Bytes, testing::TestRng};
    use futures::channel::oneshot;
    use rand::Rng;

    /// Event for the mock reactor.
    #[derive(Debug)]
    enum ReactorEvent {
        TrieAccumulatorRequest(TrieAccumulatorRequest),
        ContractRuntimeRequest(ContractRuntimeRequest),
    }

    impl From<ContractRuntimeRequest> for ReactorEvent {
        fn from(req: ContractRuntimeRequest) -> ReactorEvent {
            ReactorEvent::ContractRuntimeRequest(req)
        }
    }

    impl From<TrieAccumulatorRequest> for ReactorEvent {
        fn from(req: TrieAccumulatorRequest) -> ReactorEvent {
            ReactorEvent::TrieAccumulatorRequest(req)
        }
    }

    struct MockReactor {
        scheduler: &'static Scheduler<ReactorEvent>,
        effect_builder: EffectBuilder<ReactorEvent>,
    }

    impl MockReactor {
        fn new() -> Self {
            let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));
            let event_queue_handle = EventQueueHandle::without_shutdown(scheduler);
            let effect_builder = EffectBuilder::new(event_queue_handle);
            MockReactor {
                scheduler,
                effect_builder,
            }
        }

        fn effect_builder(&self) -> EffectBuilder<ReactorEvent> {
            self.effect_builder
        }

        async fn expect_trie_accumulator_request(&self, hash: &Digest, peers: &HashSet<NodeId>) {
            let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
            match reactor_event {
                ReactorEvent::TrieAccumulatorRequest(request) => {
                    assert_eq!(request.hash, *hash);
                    for peer in request.peers.iter() {
                        assert!(peers.contains(peer));
                    }
                }
                _ => {
                    unreachable!();
                }
            };
        }

        async fn expect_put_trie_request(&self, trie: &TrieRaw) {
            let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
            match reactor_event {
                ReactorEvent::ContractRuntimeRequest(ContractRuntimeRequest::PutTrie {
                    trie_bytes,
                    responder: _,
                }) => {
                    assert_eq!(trie_bytes, *trie);
                }
                _ => {
                    unreachable!();
                }
            };
        }
    }

    fn random_test_trie(rng: &mut TestRng) -> TrieRaw {
        let data: Vec<u8> = (0..64).into_iter().map(|_| rng.gen()).collect();
        TrieRaw::new(Bytes::from(data))
    }

    fn random_sync_global_state_request(
        rng: &mut TestRng,
        num_random_peers: usize,
        responder: Responder<Result<Digest, Error>>,
    ) -> (SyncGlobalStateRequest, TrieRaw) {
        let block = Block::random(rng); // Random block
        let trie = random_test_trie(rng);

        // Create multiple peers
        let peers: HashSet<NodeId> = (0..num_random_peers)
            .into_iter()
            .map(|_| NodeId::random(rng))
            .collect();

        // Create a request
        (
            SyncGlobalStateRequest {
                block_hash: *block.hash(),
                state_root_hash: Digest::hash(trie.inner()),
                peers,
                responder,
            },
            trie,
        )
    }

    #[tokio::test]
    async fn fetch_request_without_peers_is_canceled() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(rng.gen_range(2..10));

        // Create a responder to allow assertion of the error
        let (sender, receiver) = oneshot::channel();
        // Create a request without peers
        let (request, _) =
            random_sync_global_state_request(&mut rng, 0, Responder::without_shutdown(sender));

        // Check how the request is handled by the block synchronizer.
        // Since the request does not have any peers, it should be canceled.
        let mut effects =
            global_state_synchronizer.handle_request(request, reactor.effect_builder());
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 0);
        assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
        assert_eq!(global_state_synchronizer.in_flight.len(), 0);
        assert!(global_state_synchronizer.last_progress.is_some());

        // Check if the error is propagated on the channel
        tokio::spawn(async move { effects.remove(0).await });
        let result = receiver.await.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn sync_global_state_request_starts_maximum_trie_fetches() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        let parallel_fetch_limit = rng.gen_range(2..10);
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(parallel_fetch_limit);

        let mut progress = Timestamp::now();

        // Create multiple distinct requests up to the fetch limit
        for i in 0..parallel_fetch_limit {
            // Add a small delay between requests
            std::thread::sleep(std::time::Duration::from_millis(2));
            let (request, _) = random_sync_global_state_request(
                &mut rng,
                2,
                Responder::without_shutdown(oneshot::channel().0),
            );
            let trie_hash = request.state_root_hash;
            let peers = request.peers.clone();
            let mut effects =
                global_state_synchronizer.handle_request(request, reactor.effect_builder());
            assert_eq!(effects.len(), 1);
            assert_eq!(global_state_synchronizer.request_states.len(), i + 1);
            // Fetch should be always 0 as long as we're below parallel_fetch_limit
            assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
            assert_eq!(global_state_synchronizer.in_flight.len(), i + 1);
            assert!(global_state_synchronizer.last_progress().unwrap() > progress);
            progress = global_state_synchronizer.last_progress().unwrap();

            // Check if trie_accumulator requests were generated for all tries.
            tokio::spawn(async move { effects.remove(0).await });
            reactor
                .expect_trie_accumulator_request(&trie_hash, &peers)
                .await;
        }

        // Create another request
        let (request, _) = random_sync_global_state_request(
            &mut rng,
            2,
            Responder::without_shutdown(oneshot::channel().0),
        );

        // This request should be registered but a fetch should not be started since the limit was
        // exceeded
        let effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
        assert_eq!(effects.len(), 0);
        assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 1);
        assert_eq!(
            global_state_synchronizer.request_states.len(),
            parallel_fetch_limit + 1
        );
        assert_eq!(
            global_state_synchronizer.in_flight.len(),
            parallel_fetch_limit
        );
    }

    #[tokio::test]
    async fn trie_accumulator_error_cancels_request() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        // Set the parallel fetch limit to allow only 1 fetch
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(1);

        // Create and register one request
        let (sender, receiver) = oneshot::channel();
        let (request1, _) =
            random_sync_global_state_request(&mut rng, 2, Responder::without_shutdown(sender));
        let trie_hash1 = request1.state_root_hash;
        let peers1 = request1.peers.clone();
        let mut effects =
            global_state_synchronizer.handle_request(request1, reactor.effect_builder());
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);

        // Validate that a trie accumulator request was created
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_trie_accumulator_request(&trie_hash1, &peers1)
            .await;

        // Create and register a second request
        let (request2, _) = random_sync_global_state_request(
            &mut rng,
            2,
            Responder::without_shutdown(oneshot::channel().0),
        );
        let trie_hash2 = request2.state_root_hash;
        let peers2 = request2.peers.clone();
        let effects = global_state_synchronizer.handle_request(request2, reactor.effect_builder());
        // This request should not generate a trie fetch since the in flight limit was reached
        assert_eq!(effects.len(), 0);
        assert_eq!(global_state_synchronizer.request_states.len(), 2);
        // 2nd request is in the fetch queue
        assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 1);
        // First request is in flight
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);

        // Simulate a trie_accumulator error for the first trie
        let trie_accumulator_result = Err(TrieAccumulatorError::Absent(trie_hash1, 0));
        let mut effects = global_state_synchronizer.handle_fetched_trie(
            trie_hash1,
            trie_accumulator_result,
            reactor.effect_builder(),
        );
        assert_eq!(effects.len(), 2);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);

        let trie_fetch_effect = effects.pop().unwrap();
        let cancel_effect = effects.pop().unwrap();

        // Check if we got the error for the first trie on the channel
        tokio::spawn(async move { cancel_effect.await });
        let result = receiver.await.unwrap();
        assert!(result.is_err());

        // Validate that a trie accumulator request was created
        tokio::spawn(async move { trie_fetch_effect.await });
        reactor
            .expect_trie_accumulator_request(&trie_hash2, &peers2)
            .await;
    }

    #[tokio::test]
    async fn successful_trie_fetch_puts_trie_to_store() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(rng.gen_range(2..10));

        // Create a request
        let (request, trie) = random_sync_global_state_request(
            &mut rng,
            2,
            Responder::without_shutdown(oneshot::channel().0),
        );
        let state_root_hash = request.state_root_hash;
        let peers = request.peers.clone();

        let mut effects =
            global_state_synchronizer.handle_request(request, reactor.effect_builder());
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);
        // Validate that we got a trie_accumulator request
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_trie_accumulator_request(&state_root_hash, &peers)
            .await;

        // Simulate a successful trie fetch
        let trie_accumulator_result = Ok(Box::new(trie.clone()));
        let mut effects = global_state_synchronizer.handle_fetched_trie(
            state_root_hash,
            trie_accumulator_result,
            reactor.effect_builder(),
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 0);

        // Should attempt to put the trie to the trie store
        tokio::spawn(async move { effects.remove(0).await });
        reactor.expect_put_trie_request(&trie).await;
    }

    #[tokio::test]
    async fn trie_store_error_cancels_request() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(rng.gen_range(2..10));

        // Create a request
        let (sender, receiver) = oneshot::channel();
        let (request, trie) =
            random_sync_global_state_request(&mut rng, 2, Responder::without_shutdown(sender));
        let state_root_hash = request.state_root_hash;
        let peers = request.peers.clone();

        let mut effects =
            global_state_synchronizer.handle_request(request, reactor.effect_builder());
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);

        // Validate that we got a trie_accumulator request
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_trie_accumulator_request(&state_root_hash, &peers)
            .await;

        // Assuming we received the trie from the accumulator, check the behavior when we an error
        // is returned when trying to put the trie to the store.
        let mut effects = global_state_synchronizer.handle_put_trie_result(
            Digest::hash(trie.inner()),
            trie,
            [state_root_hash].iter().cloned().collect(),
            Err(engine_state::Error::RootNotFound(state_root_hash)),
            reactor.effect_builder(),
        );
        assert_eq!(effects.len(), 1);
        // Request should be canceled.
        assert_eq!(global_state_synchronizer.request_states.len(), 0);
        tokio::spawn(async move { effects.remove(0).await });
        let result = receiver.await.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn missing_trie_node_children_triggers_fetch() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        let parallel_fetch_limit = rng.gen_range(2..10);
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(parallel_fetch_limit);

        // Create a request
        let (request, request_trie) = random_sync_global_state_request(
            &mut rng,
            2,
            Responder::without_shutdown(oneshot::channel().0),
        );
        let state_root_hash = request.state_root_hash;
        let peers = request.peers.clone();

        let mut effects =
            global_state_synchronizer.handle_request(request, reactor.effect_builder());
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);

        // Validate that we got a trie_accumulator request
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_trie_accumulator_request(&state_root_hash, &peers)
            .await;

        // Simulate a successful trie fetch from the accumulator
        let trie_accumulator_result = Ok(Box::new(request_trie.clone()));
        let mut effects = global_state_synchronizer.handle_fetched_trie(
            state_root_hash,
            trie_accumulator_result,
            reactor.effect_builder(),
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 0);

        // Should try to put the trie in the store.
        tokio::spawn(async move { effects.remove(0).await });
        reactor.expect_put_trie_request(&request_trie).await;

        // Simulate an error from the trie store where the trie is missing children.
        // We generate more than the parallel_fetch_limit.
        let num_missing_trie_nodes = rng.gen_range(12..20);
        let missing_tries: Vec<TrieRaw> = (0..num_missing_trie_nodes)
            .into_iter()
            .map(|_| random_test_trie(&mut rng))
            .collect();
        let missing_trie_nodes_hashes: Vec<Digest> = missing_tries
            .iter()
            .map(|missing_trie| Digest::hash(missing_trie.inner()))
            .collect();

        let mut effects = global_state_synchronizer.handle_put_trie_result(
            Digest::hash(request_trie.inner()),
            request_trie.clone(),
            [state_root_hash].iter().cloned().collect(),
            Err(engine_state::Error::MissingTrieNodeChildren(
                missing_trie_nodes_hashes.clone(),
            )),
            reactor.effect_builder(),
        );

        // The global state synchronizer should now start to get the missing tries and create a
        // trie_accumulator fetch request for each of the missing children.
        assert_eq!(effects.len(), parallel_fetch_limit);
        assert_eq!(global_state_synchronizer.tries_awaiting_children.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(
            global_state_synchronizer.in_flight.len(),
            parallel_fetch_limit
        );
        // There are still tries that were not issued a fetch since it would exceed the limit.
        assert_eq!(
            global_state_synchronizer.fetch_queue.queue.len(),
            num_missing_trie_nodes - parallel_fetch_limit
        );

        // Check the requests that were issued.
        for (idx, effect) in effects.drain(0..).rev().enumerate() {
            tokio::spawn(async move { effect.await });
            reactor
                .expect_trie_accumulator_request(
                    &missing_trie_nodes_hashes[num_missing_trie_nodes - idx - 1],
                    &peers,
                )
                .await;
        }

        // Now handle a successful fetch from the trie_accumulator for one of the missing children.
        let trie_hash = missing_trie_nodes_hashes[num_missing_trie_nodes - 1];
        let trie_accumulator_result =
            Ok(Box::new(missing_tries[num_missing_trie_nodes - 1].clone()));
        let mut effects = global_state_synchronizer.handle_fetched_trie(
            trie_hash,
            trie_accumulator_result,
            reactor.effect_builder(),
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(
            global_state_synchronizer.in_flight.len(),
            parallel_fetch_limit - 1
        );
        assert_eq!(
            global_state_synchronizer.fetch_queue.queue.len(),
            num_missing_trie_nodes - parallel_fetch_limit
        );
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_put_trie_request(&missing_tries[num_missing_trie_nodes - 1])
            .await;

        // Handle put trie to store for the missing child
        let mut effects = global_state_synchronizer.handle_put_trie_result(
            trie_hash,
            missing_tries[num_missing_trie_nodes - 1].clone(),
            [state_root_hash].iter().cloned().collect(),
            Ok(trie_hash),
            reactor.effect_builder(),
        );

        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.tries_awaiting_children.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        // Check if one of the pending fetches for the missing children was picked up.
        // The in flight value should have reached the limit.
        assert_eq!(
            global_state_synchronizer.in_flight.len(),
            parallel_fetch_limit
        );
        // Should have one less missing child than before.
        assert_eq!(
            global_state_synchronizer
                .tries_awaiting_children
                .get(&Digest::hash(request_trie.inner()))
                .unwrap()
                .missing_children
                .len(),
            num_missing_trie_nodes - 1
        );

        // Check that a fetch was created for the next missing child.
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_trie_accumulator_request(
                &missing_trie_nodes_hashes[num_missing_trie_nodes - parallel_fetch_limit - 1],
                &peers,
            )
            .await;
    }

    #[tokio::test]
    async fn stored_trie_finalizes_request() {
        let mut rng = TestRng::new();
        let reactor = MockReactor::new();
        let parallel_fetch_limit = rng.gen_range(2..10);
        let mut global_state_synchronizer = GlobalStateSynchronizer::new(parallel_fetch_limit);

        // Create a request
        let (sender, receiver) = oneshot::channel();
        let (request, trie) =
            random_sync_global_state_request(&mut rng, 2, Responder::without_shutdown(sender));
        let state_root_hash = request.state_root_hash;
        let peers = request.peers.clone();

        let mut effects =
            global_state_synchronizer.handle_request(request, reactor.effect_builder());
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 1);

        // Validate that we got a trie_accumulator request
        tokio::spawn(async move { effects.remove(0).await });
        reactor
            .expect_trie_accumulator_request(&state_root_hash, &peers)
            .await;

        // Handle a successful fetch from the trie_accumulator for one of the missing children.
        let trie_hash = Digest::hash(trie.inner());
        let trie_accumulator_result = Ok(Box::new(trie.clone()));
        let mut effects = global_state_synchronizer.handle_fetched_trie(
            trie_hash,
            trie_accumulator_result,
            reactor.effect_builder(),
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.request_states.len(), 1);
        assert_eq!(global_state_synchronizer.in_flight.len(), 0);
        tokio::spawn(async move { effects.remove(0).await });
        reactor.expect_put_trie_request(&trie).await;

        // Generate a successful trie store
        let mut effects = global_state_synchronizer.handle_put_trie_result(
            trie_hash,
            trie,
            [state_root_hash].iter().cloned().collect(),
            Ok(trie_hash),
            reactor.effect_builder(),
        );
        // Check that request was successfully serviced and the global synchronizer is finished with
        // it.
        assert_eq!(effects.len(), 1);
        assert_eq!(global_state_synchronizer.tries_awaiting_children.len(), 0);
        assert_eq!(global_state_synchronizer.request_states.len(), 0);
        assert_eq!(global_state_synchronizer.in_flight.len(), 0);
        assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
        tokio::spawn(async move { effects.remove(0).await });
        let result = receiver.await.unwrap();
        assert!(result.is_ok());
    }
}
