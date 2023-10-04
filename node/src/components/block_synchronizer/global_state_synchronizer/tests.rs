use std::time::Duration;

use futures::channel::oneshot;
use rand::Rng;

use casper_types::{bytesrepr::Bytes, testing::TestRng, TestBlockBuilder};

use super::*;
use crate::{
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    utils,
};

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
        let scheduler = utils::leak(Scheduler::new(QueueKind::weights(), None));
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

    async fn expect_trie_accumulator_request(&self, hash: &Digest) {
        let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
        match reactor_event {
            ReactorEvent::TrieAccumulatorRequest(request) => {
                assert_eq!(request.hash, *hash);
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
    responder: Responder<Result<Response, Error>>,
) -> (SyncGlobalStateRequest, TrieRaw) {
    let block = TestBlockBuilder::new().build(rng);
    let trie = random_test_trie(rng);

    // Create a request
    (
        SyncGlobalStateRequest {
            block_hash: *block.hash(),
            state_root_hash: Digest::hash(trie.inner()),
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
        random_sync_global_state_request(&mut rng, Responder::without_shutdown(sender));

    // Check how the request is handled by the block synchronizer.
    let mut effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 1);
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);
    assert!(global_state_synchronizer.last_progress.is_some());

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects =
        global_state_synchronizer.parallel_fetch_with_peers(vec![], reactor.effect_builder());

    // Since the request does not have any peers, it should be canceled.
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_none());
    // Fetch should be always 0 as long as we're below parallel_fetch_limit
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    // Check if the error is propagated on the channel
    tokio::spawn(effects.remove(0));
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

    let (request, trie_raw) = random_sync_global_state_request(
        &mut rng,
        Responder::without_shutdown(oneshot::channel().0),
    );
    let trie_hash = request.state_root_hash;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let mut effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 1);
    // At first the synchronizer only fetches the root node.
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);
    assert!(global_state_synchronizer.last_progress().unwrap() > progress);
    progress = global_state_synchronizer.last_progress().unwrap();

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    // Fetch should be always 0 as long as we're below parallel_fetch_limit
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);

    // Check if trie_accumulator requests were generated for all tries.
    tokio::spawn(effects.remove(0));
    reactor.expect_trie_accumulator_request(&trie_hash).await;

    // sleep a bit so that the next progress timestamp is different
    tokio::time::sleep(Duration::from_millis(2)).await;
    // simulate the fetch returning a trie
    let effects = global_state_synchronizer.handle_fetched_trie(
        TrieHash(trie_hash),
        Ok(TrieAccumulatorResponse::new(trie_raw.clone(), vec![])),
        reactor.effect_builder(),
    );

    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    // the fetch request is no longer in flight
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);
    assert!(global_state_synchronizer.last_progress().unwrap() > progress);
    progress = global_state_synchronizer.last_progress().unwrap();

    // sleep a bit so that the next progress timestamp is different
    tokio::time::sleep(Duration::from_millis(2)).await;
    // simulate synchronizer processing the fetched trie
    let effects = global_state_synchronizer.handle_put_trie_result(
        TrieHash(trie_hash),
        trie_raw,
        // root node would have some children that we haven't yet downloaded
        Err(engine_state::Error::MissingTrieNodeChildren(
            (0u8..255)
                .into_iter()
                // TODO: generate random hashes when `rng.gen` works
                .map(|i| Digest::hash([i; 32]))
                .collect(),
        )),
        reactor.effect_builder(),
    );

    assert_eq!(effects.len(), 2);
    for effect in effects {
        let events = tokio::spawn(effect).await.unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Event::GetPeers(_)));
    }

    let effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    // The global state synchronizer should now start to get the missing tries and create a
    // trie_accumulator fetch request for each of the missing children.
    assert_eq!(effects.len(), parallel_fetch_limit);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(
        global_state_synchronizer.fetch_queue.queue.len(),
        255 - parallel_fetch_limit
    );
    assert_eq!(
        global_state_synchronizer.in_flight.len(),
        parallel_fetch_limit
    );
    assert!(global_state_synchronizer.last_progress().unwrap() > progress);
}

#[tokio::test]
async fn trie_accumulator_error_cancels_request() {
    let mut rng = TestRng::new();
    let reactor = MockReactor::new();
    // Set the parallel fetch limit to allow only 1 fetch
    let mut global_state_synchronizer = GlobalStateSynchronizer::new(1);

    // Create and register one request
    let (sender, receiver1) = oneshot::channel();
    let (request1, _) =
        random_sync_global_state_request(&mut rng, Responder::without_shutdown(sender));
    let trie_hash1 = request1.state_root_hash;
    let mut effects = global_state_synchronizer.handle_request(request1, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 1);
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);

    // Validate that a trie accumulator request was created
    tokio::spawn(effects.remove(0));
    reactor.expect_trie_accumulator_request(&trie_hash1).await;

    // Create and register a second request
    let (sender, receiver2) = oneshot::channel();
    let (request2, _) =
        random_sync_global_state_request(&mut rng, Responder::without_shutdown(sender));
    let trie_hash2 = request2.state_root_hash;
    let mut effects = global_state_synchronizer.handle_request(request2, reactor.effect_builder());
    // This request should generate an error response
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    // First request is in flight
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);

    tokio::spawn(effects.remove(0));
    match receiver2.await.unwrap() {
        // the synchronizer should say that it's already processing a different request
        Err(Error::ProcessingAnotherRequest {
            hash_being_synced,
            hash_requested,
        }) => {
            assert_eq!(hash_being_synced, trie_hash1);
            assert_eq!(hash_requested, trie_hash2);
        }
        res => panic!("unexpected result: {:?}", res),
    }

    // Simulate a trie_accumulator error for the first trie
    let trie_accumulator_result = Err(TrieAccumulatorError::Absent(trie_hash1, 0, vec![]));
    let mut effects = global_state_synchronizer.handle_fetched_trie(
        trie_hash1.into(),
        trie_accumulator_result,
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_none());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    let cancel_effect = effects.pop().unwrap();

    // Check if we got the error for the first trie on the channel
    tokio::spawn(cancel_effect);
    let result = receiver1.await.unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn successful_trie_fetch_puts_trie_to_store() {
    let mut rng = TestRng::new();
    let reactor = MockReactor::new();
    let mut global_state_synchronizer = GlobalStateSynchronizer::new(rng.gen_range(2..10));

    // Create a request
    let (request, trie) = random_sync_global_state_request(
        &mut rng,
        Responder::without_shutdown(oneshot::channel().0),
    );
    let state_root_hash = request.state_root_hash;

    let mut effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);
    // Validate that we got a trie_accumulator request
    tokio::spawn(effects.remove(0));
    reactor
        .expect_trie_accumulator_request(&state_root_hash)
        .await;

    // Simulate a successful trie fetch
    let trie_accumulator_result = Ok(TrieAccumulatorResponse::new(trie.clone(), Vec::new()));
    let mut effects = global_state_synchronizer.handle_fetched_trie(
        state_root_hash.into(),
        trie_accumulator_result,
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    // Should attempt to put the trie to the trie store
    tokio::spawn(effects.remove(0));
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
        random_sync_global_state_request(&mut rng, Responder::without_shutdown(sender));
    let state_root_hash = request.state_root_hash;

    let mut effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);

    // Validate that we got a trie_accumulator request
    tokio::spawn(effects.remove(0));
    reactor
        .expect_trie_accumulator_request(&state_root_hash)
        .await;

    // Assuming we received the trie from the accumulator, check the behavior when we an error
    // is returned when trying to put the trie to the store.
    let mut effects = global_state_synchronizer.handle_put_trie_result(
        Digest::hash(trie.inner()).into(),
        trie,
        Err(engine_state::Error::RootNotFound(state_root_hash)),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    // Request should be canceled.
    assert!(global_state_synchronizer.request_state.is_none());
    tokio::spawn(effects.remove(0));
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
        Responder::without_shutdown(oneshot::channel().0),
    );
    let state_root_hash = request.state_root_hash;

    let mut effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);

    // Validate that we got a trie_accumulator request
    tokio::spawn(effects.remove(0));
    reactor
        .expect_trie_accumulator_request(&state_root_hash)
        .await;

    // Simulate a successful trie fetch from the accumulator
    let trie_accumulator_result = Ok(TrieAccumulatorResponse::new(
        request_trie.clone(),
        Vec::new(),
    ));
    let mut effects = global_state_synchronizer.handle_fetched_trie(
        state_root_hash.into(),
        trie_accumulator_result,
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    // Should try to put the trie in the store.
    tokio::spawn(effects.remove(0));
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

    let effects = global_state_synchronizer.handle_put_trie_result(
        Digest::hash(request_trie.inner()).into(),
        request_trie.clone(),
        Err(engine_state::Error::MissingTrieNodeChildren(
            missing_trie_nodes_hashes.clone(),
        )),
        reactor.effect_builder(),
    );

    assert_eq!(effects.len(), 2);
    for effect in effects {
        let events = tokio::spawn(effect).await.unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Event::GetPeers(_)));
    }

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    // The global state synchronizer should now start to get the missing tries and create a
    // trie_accumulator fetch request for each of the missing children.
    assert_eq!(effects.len(), parallel_fetch_limit);
    assert_eq!(global_state_synchronizer.tries_awaiting_children.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
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
        tokio::spawn(effect);
        reactor
            .expect_trie_accumulator_request(
                &missing_trie_nodes_hashes[num_missing_trie_nodes - idx - 1],
            )
            .await;
    }

    // Now handle a successful fetch from the trie_accumulator for one of the missing children.
    let trie_hash = missing_trie_nodes_hashes[num_missing_trie_nodes - 1];
    let trie_accumulator_result = Ok(TrieAccumulatorResponse::new(
        missing_tries[num_missing_trie_nodes - 1].clone(),
        Vec::new(),
    ));
    let mut effects = global_state_synchronizer.handle_fetched_trie(
        trie_hash.into(),
        trie_accumulator_result,
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(
        global_state_synchronizer.in_flight.len(),
        parallel_fetch_limit - 1
    );
    assert_eq!(
        global_state_synchronizer.fetch_queue.queue.len(),
        num_missing_trie_nodes - parallel_fetch_limit
    );
    tokio::spawn(effects.remove(0));
    reactor
        .expect_put_trie_request(&missing_tries[num_missing_trie_nodes - 1])
        .await;

    // Handle put trie to store for the missing child
    let mut effects = global_state_synchronizer.handle_put_trie_result(
        trie_hash.into(),
        missing_tries[num_missing_trie_nodes - 1].clone(),
        Ok(trie_hash.into()),
        reactor.effect_builder(),
    );

    assert_eq!(effects.len(), 1);
    assert_eq!(global_state_synchronizer.tries_awaiting_children.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    // The in flight value should still be 1 below the limit - the effects should contain a request
    // for peers.
    assert_eq!(
        global_state_synchronizer.in_flight.len(),
        parallel_fetch_limit - 1
    );

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    // Check if one of the pending fetches for the missing children was picked up.
    assert_eq!(
        global_state_synchronizer.in_flight.len(),
        parallel_fetch_limit
    );

    // Should have one less missing child than before.
    assert_eq!(
        global_state_synchronizer
            .tries_awaiting_children
            .get(&Digest::hash(request_trie.inner()).into())
            .unwrap()
            .missing_children
            .len(),
        num_missing_trie_nodes - 1
    );

    // Check that a fetch was created for the next missing child.
    tokio::spawn(effects.remove(0));
    reactor
        .expect_trie_accumulator_request(
            &missing_trie_nodes_hashes[num_missing_trie_nodes - parallel_fetch_limit - 1],
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
        random_sync_global_state_request(&mut rng, Responder::without_shutdown(sender));
    let state_root_hash = request.state_root_hash;

    let mut effects = global_state_synchronizer.handle_request(request, reactor.effect_builder());
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);

    let events = tokio::spawn(effects.remove(0)).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], Event::GetPeers(_)));

    let mut effects = global_state_synchronizer.parallel_fetch_with_peers(
        std::iter::repeat_with(|| NodeId::random(&mut rng))
            .take(2)
            .collect(),
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 1);

    // Validate that we got a trie_accumulator request
    tokio::spawn(effects.remove(0));
    reactor
        .expect_trie_accumulator_request(&state_root_hash)
        .await;

    // Handle a successful fetch from the trie_accumulator for one of the missing children.
    let trie_hash = Digest::hash(trie.inner());
    let trie_accumulator_result = Ok(TrieAccumulatorResponse::new(trie.clone(), Vec::new()));
    let mut effects = global_state_synchronizer.handle_fetched_trie(
        trie_hash.into(),
        trie_accumulator_result,
        reactor.effect_builder(),
    );
    assert_eq!(effects.len(), 1);
    assert!(global_state_synchronizer.request_state.is_some());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);
    tokio::spawn(effects.remove(0));
    reactor.expect_put_trie_request(&trie).await;

    // Generate a successful trie store
    let mut effects = global_state_synchronizer.handle_put_trie_result(
        trie_hash.into(),
        trie,
        Ok(trie_hash.into()),
        reactor.effect_builder(),
    );
    // Check that request was successfully serviced and the global synchronizer is finished with
    // it.
    assert_eq!(effects.len(), 1);
    assert_eq!(global_state_synchronizer.tries_awaiting_children.len(), 0);
    assert!(global_state_synchronizer.request_state.is_none());
    assert_eq!(global_state_synchronizer.in_flight.len(), 0);
    assert_eq!(global_state_synchronizer.fetch_queue.queue.len(), 0);
    tokio::spawn(effects.remove(0));
    let result = receiver.await.unwrap();
    assert!(result.is_ok());
}
