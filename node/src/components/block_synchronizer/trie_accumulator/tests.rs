use super::*;
use crate::{
    components::block_synchronizer::tests::test_utils::test_chunks_with_proof,
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    types::ValueOrChunk,
    utils,
};
use casper_types::testing::TestRng;
use futures::channel::oneshot;

/// Event for the mock reactor.
#[derive(Debug)]
enum ReactorEvent {
    FetcherRequest(FetcherRequest<TrieOrChunk>),
    PeerBehaviorAnnouncement(PeerBehaviorAnnouncement),
}

impl From<PeerBehaviorAnnouncement> for ReactorEvent {
    fn from(req: PeerBehaviorAnnouncement) -> ReactorEvent {
        ReactorEvent::PeerBehaviorAnnouncement(req)
    }
}

impl From<FetcherRequest<TrieOrChunk>> for ReactorEvent {
    fn from(req: FetcherRequest<TrieOrChunk>) -> ReactorEvent {
        ReactorEvent::FetcherRequest(req)
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

    async fn expect_fetch_event(&self, chunk_id: &TrieOrChunkId, peer: &NodeId) {
        let ((_ancestor, reactor_event), _) = self.scheduler.pop().await;
        match reactor_event {
            ReactorEvent::FetcherRequest(request) => {
                assert_eq!(request.id, *chunk_id);
                assert_eq!(request.peer, *peer);
            }
            _ => {
                unreachable!();
            }
        };
    }
}

async fn download_chunk_and_check(
    reactor: &MockReactor,
    trie_accumulator: &mut TrieAccumulator,
    chunk_to_download: &TrieOrChunkId,
    peer: &NodeId,
    partial_chunks: PartialChunks,
) {
    // Try to download a chunk from a peer
    let mut effects = trie_accumulator.try_download_chunk(
        reactor.effect_builder(),
        *chunk_to_download,
        *peer,
        partial_chunks,
    );
    // A fetch effect should be generated
    assert_eq!(effects.len(), 1);

    // Run the effects and check if the correct fetch was requested
    tokio::spawn(async move { effects.remove(0).await });
    reactor.expect_fetch_event(chunk_to_download, peer).await;
}

#[test]
fn unsolicited_chunk_produces_no_effects() {
    let reactor = MockReactor::new();

    // Empty accumulator. Does not expect any chunks.
    let mut trie_accumulator = TrieAccumulator::new();
    let (test_chunks, _, _) = test_chunks_with_proof(1);

    let effects = trie_accumulator.consume_chunk(reactor.effect_builder(), test_chunks[0].clone());
    assert!(effects.is_empty());
}

#[tokio::test]
async fn try_download_chunk_generates_fetch_effect() {
    let mut rng = TestRng::new();
    let reactor = MockReactor::new();
    let mut trie_accumulator = TrieAccumulator::new();

    // Create a test chunk
    let (_, chunk_ids, _) = test_chunks_with_proof(1);
    let peer = NodeId::random(&mut rng);
    let chunks = PartialChunks {
        peers: vec![peer],
        responders: Default::default(),
        chunks: Default::default(),
        unreliable_peers: Default::default(),
    };

    download_chunk_and_check(
        &reactor,
        &mut trie_accumulator,
        &chunk_ids[0],
        &peer,
        chunks,
    )
    .await;
}

#[tokio::test]
async fn failed_fetch_retriggers_download_with_different_peer() {
    let mut rng = TestRng::new();
    let reactor = MockReactor::new();
    let mut trie_accumulator = TrieAccumulator::new();

    // Create a test chunk
    let (_, chunk_ids, _) = test_chunks_with_proof(1);

    // Create multiple peers
    let peers: Vec<NodeId> = (0..2)
        .into_iter()
        .map(|_| NodeId::random(&mut rng))
        .collect();

    let chunks = PartialChunks {
        peers: peers.clone(),
        responders: Default::default(),
        chunks: Default::default(),
        unreliable_peers: Default::default(),
    };

    download_chunk_and_check(
        &reactor,
        &mut trie_accumulator,
        &chunk_ids[0],
        &peers[1],
        chunks,
    )
    .await;

    // Simulate a fetch error
    let fetch_result: FetchResult<TrieOrChunk> = Err(FetcherError::TimedOut {
        id: Box::new(chunk_ids[0]),
        peer: peers[1],
    });
    let event = Event::TrieOrChunkFetched {
        id: chunk_ids[0],
        fetch_result,
    };

    // Handling the fetch error should make the trie accumulator generate another fetch for the
    // same chunk but with a different peer
    let mut effects = trie_accumulator.handle_event(reactor.effect_builder(), &mut rng, event);
    assert_eq!(effects.len(), 1);

    // Run the effects and check if the fetch was re-triggered
    tokio::spawn(async move { effects.remove(0).await });
    reactor.expect_fetch_event(&chunk_ids[0], &peers[0]).await;
}

#[tokio::test]
async fn fetched_chunk_triggers_download_of_missing_chunk() {
    let mut rng = TestRng::new();
    let reactor = MockReactor::new();
    let mut trie_accumulator = TrieAccumulator::new();

    // Create test chunks
    let (test_chunks, chunk_ids, _) = test_chunks_with_proof(2);
    let peer = NodeId::random(&mut rng);

    let chunks = PartialChunks {
        peers: vec![peer],
        responders: Default::default(),
        chunks: Default::default(),
        unreliable_peers: Default::default(),
    };

    download_chunk_and_check(
        &reactor,
        &mut trie_accumulator,
        &chunk_ids[1],
        &peer,
        chunks,
    )
    .await;

    // Simulate a successful fetch
    let chunk = Box::new(ValueOrChunk::ChunkWithProof(test_chunks[1].clone()));
    let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer { peer, item: chunk });
    let event = Event::TrieOrChunkFetched {
        id: chunk_ids[1],
        fetch_result,
    };

    // Process the downloaded chunk
    let mut effects = trie_accumulator.handle_event(reactor.effect_builder(), &mut rng, event);
    assert_eq!(effects.len(), 1);

    // Check if a new fetch was issued for the missing chunk
    tokio::spawn(async move { effects.remove(0).await });
    reactor.expect_fetch_event(&chunk_ids[0], &peer).await;
}

#[tokio::test]
async fn trie_returned_when_all_chunks_fetched() {
    let mut rng = TestRng::new();
    let reactor = MockReactor::new();
    let mut trie_accumulator = TrieAccumulator::new();

    // Create test chunks
    let (test_chunks, chunk_ids, data) = test_chunks_with_proof(3);
    let peer = NodeId::random(&mut rng);

    // Create a responder to assert the validity of the assembled trie
    let (sender, receiver) = oneshot::channel();
    let responder = Responder::without_shutdown(sender);

    let chunks = PartialChunks {
        peers: vec![peer],
        responders: vec![responder],
        chunks: Default::default(),
        unreliable_peers: Default::default(),
    };

    download_chunk_and_check(
        &reactor,
        &mut trie_accumulator,
        &chunk_ids[0],
        &peer,
        chunks,
    )
    .await;

    let mut effects = Effects::new();

    for i in 0..3 {
        // Simulate a successful fetch
        let fetch_result: FetchResult<TrieOrChunk> = Ok(FetchedData::FromPeer {
            peer,
            item: Box::new(ValueOrChunk::ChunkWithProof(test_chunks[i].clone())),
        });
        let event = Event::TrieOrChunkFetched {
            id: chunk_ids[i],
            fetch_result,
        };

        // Expect to get one effect for each call. First 2 will be requests to download missing
        // chunks. Last one will be the returned trie since all chunks are available.
        effects = trie_accumulator.handle_event(reactor.effect_builder(), &mut rng, event);
        assert_eq!(effects.len(), 1);
    }

    // Validate the returned trie
    tokio::spawn(async move { effects.remove(0).await });
    let result_trie = receiver.await.unwrap().expect("Expected trie").trie;
    assert_eq!(*result_trie, TrieRaw::new(Bytes::from(data)));
}
