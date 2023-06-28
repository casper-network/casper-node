#[cfg(test)]
mod tests;

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

use casper_storage::global_state::trie::TrieRaw;
use casper_types::{bytesrepr::Bytes, ChunkWithProof, Digest, DisplayIter};

use crate::{
    components::{
        fetcher::{
            EmptyValidationMetadata, Error as FetcherError, FetchItem, FetchResult, FetchedData,
        },
        Component,
    },
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{FetcherRequest, TrieAccumulatorRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{NodeId, TrieOrChunk, TrieOrChunkId},
    NodeRng,
};

const COMPONENT_NAME: &str = "trie_accumulator";

#[derive(Debug, From, Error, Clone, Serialize)]
pub(crate) enum Error {
    #[error("trie accumulator ran out of peers trying to fetch item with error: {0}; unreliable peers: {}", DisplayIter::new(.1))]
    // Note: Due to being a thrice nested component, this error type tighter size constraints. For
    //       this reason, we have little choice but to box the `FetcherError`.
    PeersExhausted(Box<FetcherError<TrieOrChunk>>, Vec<NodeId>),
    #[error("trie accumulator couldn't fetch trie chunk ({0}, {1}); unreliable peers: {}", DisplayIter::new(.2))]
    Absent(Digest, u64, Vec<NodeId>),
    #[error("request contained no peers; trie = {0}")]
    NoPeers(Digest),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Response {
    trie: Box<TrieRaw>,
    unreliable_peers: Vec<NodeId>,
}

impl Response {
    pub(crate) fn new(trie: TrieRaw, unreliable_peers: Vec<NodeId>) -> Self {
        Response {
            trie: Box::new(trie),
            unreliable_peers,
        }
    }

    pub(crate) fn trie(self) -> Box<TrieRaw> {
        self.trie
    }

    pub(crate) fn unreliable_peers(&self) -> &Vec<NodeId> {
        &self.unreliable_peers
    }
}

#[derive(DataSize, Debug)]
struct PartialChunks {
    peers: Vec<NodeId>,
    responders: Vec<Responder<Result<Response, Error>>>,
    chunks: HashMap<u64, ChunkWithProof>,
    unreliable_peers: Vec<NodeId>,
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

    fn respond(self, value: Result<Response, Error>) -> Effects<Event> {
        self.responders
            .into_iter()
            .flat_map(|responder| responder.respond(value.clone()).ignore())
            .collect()
    }

    fn mark_peer_unreliable(&mut self, peer: &NodeId) {
        self.unreliable_peers.push(*peer);
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
        let TrieOrChunkId(_index, hash) = trie_or_chunk.fetch_id();
        match trie_or_chunk {
            TrieOrChunk::Value(trie) => match self.partial_chunks.remove(&hash) {
                None => {
                    error!(%hash, "fetched a trie we didn't request!");
                    Effects::new()
                }
                Some(partial_chunks) => {
                    trace!(%hash, "got a full trie");
                    let unreliable_peers = partial_chunks.unreliable_peers.clone();
                    partial_chunks.respond(Ok(Response::new(trie.into_inner(), unreliable_peers)))
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
                        let unreliable_peers = partial_chunks.unreliable_peers.clone();
                        return partial_chunks.respond(Err(Error::Absent(
                            digest,
                            index,
                            unreliable_peers,
                        )));
                    }
                };
                let next_id = TrieOrChunkId(missing_index, digest);
                self.try_download_chunk(effect_builder, next_id, peer, partial_chunks)
            }
            None => {
                let trie = partial_chunks.assemble_chunks(count);
                let unreliable_peers = partial_chunks.unreliable_peers.clone();
                partial_chunks.respond(Ok(Response::new(trie, unreliable_peers)))
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
            .fetch::<TrieOrChunk>(id, peer, Box::new(EmptyValidationMetadata))
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
                    unreliable_peers: Vec::new(),
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
                            partial_chunks.mark_peer_unreliable(error.peer());
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
                                    let faulty_peers = partial_chunks.unreliable_peers.clone();
                                    partial_chunks.respond(Err(Error::PeersExhausted(
                                        Box::new(error),
                                        faulty_peers,
                                    )))
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
