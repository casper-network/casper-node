use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug},
};

use datasize::DataSize;
use derive_more::From;
use rand::seq::SliceRandom;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, trace, warn};

use casper_execution_engine::storage::trie::TrieRaw;
use casper_hashing::{ChunkWithProof, Digest};
use casper_types::bytesrepr::{self, Bytes};

use crate::{
    components::{
        fetcher::{Error as FetcherError, FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{FetcherRequest, TrieAccumulatorRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{EmptyValidationMetadata, Item, NodeId, TrieOrChunk, TrieOrChunkId},
    NodeRng,
};

const COMPONENT_NAME: &str = "trie_accumulator";

#[derive(Debug, From, Error, Clone, Serialize)]
pub(crate) enum Error {
    #[error("trie accumulator fetcher error: {0}")]
    Fetcher(FetcherError<TrieOrChunk>),
    #[error("trie accumulator serialization error: {0}")]
    Bytesrepr(bytesrepr::Error),
    #[error("trie accumulator couldn't fetch trie chunk ({0}, {1})")]
    Absent(Digest, u64),
    #[error("request contained no peers; trie = {0}")]
    NoPeers(Digest),
}

#[derive(DataSize, Debug)]
struct PartialChunks {
    peers: Vec<NodeId>,
    responders: Vec<Responder<Result<Box<TrieRaw>, Error>>>,
    chunks: HashMap<u64, ChunkWithProof>,
}

impl PartialChunks {
    fn missing_chunk(&self, count: u64) -> Option<u64> {
        (0..count).find(|idx| !self.chunks.contains_key(idx))
    }

    fn assemble_chunks(&self, count: u64) -> TrieRaw {
        let data: Bytes = (0..count)
            .filter_map(|index| self.chunks.get(&index))
            .flat_map(|chunk| chunk.chunk())
            .copied()
            .collect();
        TrieRaw::new(data)
    }

    fn next_peer(&mut self) -> Option<&NodeId> {
        // remove the last used peer from the queue
        self.peers.pop();
        self.peers.last()
    }

    fn merge(&mut self, other: PartialChunks) {
        self.chunks.extend(other.chunks);
        self.responders.extend(other.responders);
        // set used for filtering out duplicates
        let mut filter_peers: HashSet<NodeId> = self.peers.iter().cloned().collect();
        for peer in other.peers {
            if filter_peers.insert(peer) {
                self.peers.push(peer);
            }
        }
    }

    fn respond(self, value: Result<Box<TrieRaw>, Error>) -> Effects<Event> {
        self.responders
            .into_iter()
            .flat_map(|responder| responder.respond(value.clone()).ignore())
            .collect()
    }
}

#[derive(DataSize, Debug)]
pub(super) struct TrieAccumulator {
    partial_chunks: HashMap<Digest, PartialChunks>,
}

#[derive(DataSize, Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    Request(TrieAccumulatorRequest),
    TrieOrChunkFetched {
        id: TrieOrChunkId,
        fetch_result: FetchResult<TrieOrChunk>,
    },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::Request(_) => write!(f, "trie fetcher request"),
            Event::TrieOrChunkFetched { id, .. } => {
                write!(f, "got a result for trie or chunk {}", id)
            }
        }
    }
}

impl TrieAccumulator {
    pub(crate) fn new() -> Self {
        TrieAccumulator {
            partial_chunks: Default::default(),
        }
    }

    fn consume_trie_or_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        trie_or_chunk: TrieOrChunk,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<TrieOrChunk>> + From<PeerBehaviorAnnouncement> + Send,
    {
        let TrieOrChunkId(_index, hash) = trie_or_chunk.id();
        match trie_or_chunk {
            TrieOrChunk::Value(trie) => match self.partial_chunks.remove(&hash) {
                None => {
                    error!(%hash, "fetched a trie we didn't request!");
                    Effects::new()
                }
                Some(partial_chunks) => {
                    debug!(%hash, "got a full trie");
                    partial_chunks.respond(Ok(Box::new(trie.into_inner())))
                }
            },
            TrieOrChunk::ChunkWithProof(chunk) => self.consume_chunk(effect_builder, chunk),
        }
    }

    fn consume_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        chunk: ChunkWithProof,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<TrieOrChunk>> + From<PeerBehaviorAnnouncement> + Send,
    {
        let digest = chunk.proof().root_hash();
        let index = chunk.proof().index();
        let count = chunk.proof().count();
        let mut partial_chunks = match self.partial_chunks.remove(&digest) {
            None => {
                error!(%digest, %index, "got a chunk that wasn't requested");
                return Effects::new();
            }
            Some(partial_chunks) => partial_chunks,
        };

        // Add the downloaded chunk to cache.
        let _ = partial_chunks.chunks.insert(index, chunk);

        // Check if we can now return a complete trie.
        match partial_chunks.missing_chunk(count) {
            Some(missing_index) => {
                let peer = match partial_chunks.peers.last() {
                    Some(peer) => *peer,
                    None => {
                        debug!(
                            %digest, %missing_index,
                            "no peers to download the next chunk from, giving up",
                        );
                        return partial_chunks.respond(Err(Error::Absent(digest, index)));
                    }
                };
                let next_id = TrieOrChunkId(missing_index, digest);
                self.try_download_chunk(effect_builder, next_id, peer, partial_chunks)
            }
            None => {
                let trie = partial_chunks.assemble_chunks(count);
                partial_chunks.respond(Ok(Box::new(trie)))
            }
        }
    }

    fn try_download_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: TrieOrChunkId,
        peer: NodeId,
        partial_chunks: PartialChunks,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<TrieOrChunk>> + Send,
    {
        let hash = id.digest();
        let maybe_old_partial_chunks = self.partial_chunks.insert(*hash, partial_chunks);
        if let Some(old_partial_chunks) = maybe_old_partial_chunks {
            // unwrap is safe as we just inserted a value at this key
            self.partial_chunks
                .get_mut(hash)
                .unwrap()
                .merge(old_partial_chunks);
        }
        effect_builder
            .fetch::<TrieOrChunk>(id, peer, EmptyValidationMetadata)
            .event(move |fetch_result| Event::TrieOrChunkFetched { id, fetch_result })
    }
}

impl<REv> Component<REv> for TrieAccumulator
where
    REv: From<FetcherRequest<TrieOrChunk>> + From<PeerBehaviorAnnouncement> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!(?event, "TrieAccumulator: handling event");
        match event {
            Event::Request(TrieAccumulatorRequest {
                hash,
                responder,
                mut peers,
            }) => {
                peers.shuffle(rng);
                let trie_id = TrieOrChunkId(0, hash);
                let peer = match peers.last() {
                    Some(peer) => *peer,
                    None => {
                        error!(%hash, "tried to fetch trie with no peers available");
                        return responder.respond(Err(Error::NoPeers(hash))).ignore();
                    }
                };
                let partial_chunks = PartialChunks {
                    responders: vec![responder],
                    peers,
                    chunks: Default::default(),
                };
                self.try_download_chunk(effect_builder, trie_id, peer, partial_chunks)
            }
            Event::TrieOrChunkFetched { id, fetch_result } => {
                let hash = id.digest();
                match fetch_result {
                    Err(error) => match self.partial_chunks.remove(hash) {
                        None => {
                            error!(%id,
                                "got a fetch result for a chunk we weren't trying to fetch",
                            );
                            Effects::new()
                        }
                        Some(mut partial_chunks) => {
                            debug!(%error, %id, "error fetching trie chunk");
                            // try with the next peer, if possible
                            match partial_chunks.next_peer().cloned() {
                                Some(next_peer) => self.try_download_chunk(
                                    effect_builder,
                                    id,
                                    next_peer,
                                    partial_chunks,
                                ),
                                None => {
                                    warn!(%id, "couldn't fetch chunk");
                                    partial_chunks.respond(Err(error.into()))
                                }
                            }
                        }
                    },
                    Ok(FetchedData::FromStorage {
                        item: trie_or_chunk,
                    }) => {
                        debug!(%trie_or_chunk, "got trie or chunk from storage");
                        self.consume_trie_or_chunk(effect_builder, *trie_or_chunk)
                    }
                    Ok(FetchedData::FromPeer {
                        item: trie_or_chunk,
                        peer,
                    }) => {
                        debug!(%peer, %trie_or_chunk, "got trie or chunk from peer");
                        self.consume_trie_or_chunk(effect_builder, *trie_or_chunk)
                    }
                }
            }
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
        types::ValueOrChunk,
        utils,
    };
    use casper_types::testing::TestRng;
    use futures::channel::oneshot;
    use rand::Rng;

    fn test_chunks_with_proof(
        num_chunks: u64,
    ) -> (Vec<ChunkWithProof>, Vec<TrieOrChunkId>, Vec<u8>) {
        let mut rng = rand::thread_rng();
        let data: Vec<u8> = (0..ChunkWithProof::CHUNK_SIZE_BYTES * num_chunks as usize)
            .into_iter()
            .map(|_| rng.gen())
            .collect();

        let chunks: Vec<ChunkWithProof> = (0..num_chunks)
            .into_iter()
            .map(|index| ChunkWithProof::new(&data, index).unwrap())
            .collect();

        let chunk_ids: Vec<TrieOrChunkId> = (0..num_chunks)
            .into_iter()
            .map(|index| TrieOrChunkId(index, chunks[index as usize].proof().root_hash()))
            .collect();

        (chunks, chunk_ids, data)
    }

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

        let effects =
            trie_accumulator.consume_chunk(reactor.effect_builder(), test_chunks[0].clone());
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
            id: chunk_ids[0],
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
        let fetch_result: FetchResult<TrieOrChunk> =
            Ok(FetchedData::FromPeer { peer, item: chunk });
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
        let (test_chunks, chunk_ids, data) = test_chunks_with_proof(rng.gen_range(2..10));
        let peer = NodeId::random(&mut rng);

        // Create a responder to assert the validity of the assembled trie
        let (sender, receiver) = oneshot::channel();
        let responder = Responder::without_shutdown(sender);

        let chunks = PartialChunks {
            peers: vec![peer],
            responders: vec![responder],
            chunks: Default::default(),
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
        let result_trie = receiver.await.unwrap().expect("Expected trie");
        assert_eq!(*result_trie, TrieRaw::new(Bytes::from(data)));
    }
}
