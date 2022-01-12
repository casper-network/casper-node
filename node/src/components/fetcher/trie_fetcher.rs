use std::{
    collections::HashMap,
    fmt::{self, Debug},
    mem,
};

use datasize::DataSize;
use derive_more::From;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_execution_engine::storage::trie::{Trie, TrieOrChunk, TrieOrChunkId};
use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{bytesrepr, Key, StoredValue};

use crate::{
    components::{
        fetcher::{
            event::{FetchResult, FetchedData, FetcherError},
            ReactorEventT,
        },
        Component,
    },
    effect::{
        announcements::{BlocklistAnnouncement, ControlAnnouncement},
        requests::{FetcherRequest, TrieFetcherRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::Item,
    NodeRng,
};

#[derive(Debug, From, Error, Clone)]
pub(crate) enum TrieFetcherError<I>
where
    I: Debug + Eq + Clone,
{
    #[error("Fetcher error: {0}")]
    Fetcher(FetcherError<TrieOrChunk, I>),
    #[error("Serialization error: {0}")]
    Bytesrepr(bytesrepr::Error),
    #[error("Couldn't fetch trie chunk ({0}, {1})")]
    Absent(Digest, u64),
}

pub(crate) type TrieFetcherResult<I> =
    Result<FetchedData<Trie<Key, StoredValue>, I>, TrieFetcherError<I>>;

#[derive(DataSize, Debug)]
pub(crate) struct PartialChunks<I>
where
    I: Debug + Eq + Clone,
{
    peers: Vec<I>,
    responders: Vec<Responder<TrieFetcherResult<I>>>,
    chunks: HashMap<u64, ChunkWithProof>,
    sender: Option<I>,
}

impl<I> PartialChunks<I>
where
    I: Debug + Eq + Clone + Send + 'static,
{
    fn missing_chunk(&self, count: u64) -> Option<u64> {
        (0..count).find(|idx| !self.chunks.contains_key(idx))
    }

    fn mutate_sender(&mut self, sender: Option<I>) {
        let old_sender = self.sender.take();
        self.sender = old_sender.or(sender);
    }

    fn assemble_chunks(&self, count: u64) -> Result<Trie<Key, StoredValue>, bytesrepr::Error> {
        let data: Vec<u8> = (0..count)
            .filter_map(|index| self.chunks.get(&index))
            .flat_map(|chunk| chunk.chunk())
            .copied()
            .collect();
        bytesrepr::deserialize(data)
    }

    fn next_peer(&mut self, prev_peer: I) -> Option<&I> {
        // return None early if not found - it means we didn't even request the chunk from the
        // previous peer
        let index = self
            .peers
            .iter()
            .enumerate()
            .rev() // to optimize slightly for the happy path: we expect prev_peer to be the last
            .find(|(_index, peer)| **peer == prev_peer)
            .map(|(index, _)| index)?;
        self.peers.remove(index);
        self.peers.last()
    }

    fn merge(&mut self, other: PartialChunks<I>) {
        self.chunks.extend(other.chunks);
        self.responders.extend(other.responders);
        self.peers.extend(other.peers);
        self.sender = self.sender.take().or(other.sender);
    }

    fn respond(&mut self, value: TrieFetcherResult<I>) -> Effects<Event<I>> {
        mem::take(&mut self.responders)
            .into_iter()
            .flat_map(|responder| responder.respond(value.clone()).ignore())
            .collect()
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct TrieFetcher<I>
where
    I: Debug + Eq + Clone,
{
    partial_chunks: HashMap<Digest, PartialChunks<I>>,
}

#[derive(DataSize, Debug, From)]
pub(crate) enum Event<I>
where
    I: Debug + Eq + Clone,
{
    #[from]
    Request(TrieFetcherRequest<I>),
    TrieOrChunkFetched {
        id: TrieOrChunkId,
        fetch_result: FetchResult<TrieOrChunk, I>,
    },
}

impl<I> fmt::Display for Event<I>
where
    I: Debug + Eq + Clone,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::Request(_) => write!(f, "trie fetcher request"),
            Event::TrieOrChunkFetched { id, .. } => {
                write!(f, "got a result for trie or chunk {}", id)
            }
        }
    }
}

fn into_response<I>(trie: Box<Trie<Key, StoredValue>>, sender: Option<I>) -> TrieFetcherResult<I>
where
    I: Debug + Eq + Clone,
{
    match sender {
        Some(peer) => Ok(FetchedData::FromPeer { item: trie, peer }),
        None => Ok(FetchedData::FromStorage { item: trie }),
    }
}

impl<I> TrieFetcher<I>
where
    I: Debug + Clone + Send + Eq + 'static,
{
    pub(crate) fn new() -> Self {
        TrieFetcher {
            partial_chunks: Default::default(),
        }
    }

    fn consume_trie_or_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sender: Option<I>,
        trie_or_chunk: TrieOrChunk,
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<TrieOrChunk>
            + From<FetcherRequest<I, TrieOrChunk>>
            + From<ControlAnnouncement>
            + From<BlocklistAnnouncement<I>>,
    {
        let TrieOrChunkId(_index, hash) = trie_or_chunk.id();
        match trie_or_chunk {
            TrieOrChunk::Trie(trie) => match self.partial_chunks.remove(&hash) {
                None => {
                    error!(%hash, "fetched a trie we didn't request!");
                    Effects::new()
                }
                Some(mut partial_chunks) => {
                    debug!(%hash, "got a full trie");
                    partial_chunks.respond(into_response(trie, sender))
                }
            },
            TrieOrChunk::ChunkWithProof(chunk) => self.consume_chunk(effect_builder, sender, chunk),
        }
    }

    fn next_peer(&mut self, id: TrieOrChunkId, prev_peer: I) -> Option<I> {
        let hash = id.digest();
        self.partial_chunks
            .get_mut(hash)
            .and_then(|partial_chunks| partial_chunks.next_peer(prev_peer).cloned())
    }

    fn consume_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sender: Option<I>,
        chunk: ChunkWithProof,
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<TrieOrChunk>
            + From<FetcherRequest<I, TrieOrChunk>>
            + From<ControlAnnouncement>
            + From<BlocklistAnnouncement<I>>,
    {
        if !chunk.verify() {
            match sender {
                None => {
                    return fatal!(effect_builder, "got an invalid chunk from storage").ignore();
                }
                Some(sender) => {
                    warn!(?sender, ?chunk, "got an invalid chunk from sender");
                    let id = TrieOrChunkId(chunk.proof().index(), chunk.proof().root_hash());
                    let mut effects = effect_builder
                        .announce_disconnect_from_peer(sender.clone())
                        .ignore();
                    if let Some(peer) = self.next_peer(id, sender) {
                        effects.extend(effect_builder.fetch(id, peer).event(move |fetch_result| {
                            Event::TrieOrChunkFetched { id, fetch_result }
                        }));
                    }
                    return effects;
                }
            }
        }
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
        // If it was downloaded from a peer, save the information.
        partial_chunks.mutate_sender(sender);

        // Check if we can now return a complete trie.
        match partial_chunks.missing_chunk(count) {
            Some(missing_index) => {
                let peer = match partial_chunks.peers.last() {
                    Some(peer) => peer.clone(),
                    None => {
                        debug!(
                            %digest, %missing_index,
                            "no peers to download the next chunk from, giving up",
                        );
                        return partial_chunks
                            .respond(Err(TrieFetcherError::Absent(digest, index)));
                    }
                };
                let next_id = TrieOrChunkId(missing_index, digest);
                self.try_download_chunk(effect_builder, next_id, peer, partial_chunks)
            }
            None => match partial_chunks.assemble_chunks(count) {
                Ok(trie) => {
                    let sender = partial_chunks.sender.clone();
                    partial_chunks.respond(into_response(Box::new(trie), sender))
                }
                Err(error) => {
                    error!(%digest, %error,
                        "error while assembling a complete trie",
                    );
                    let mut effects = partial_chunks.respond(Err(error.into()));
                    effects.extend(
                        fatal!(
                            effect_builder,
                            "cryptographically verified data failed to deserialize"
                        )
                        .ignore(),
                    );
                    effects
                }
            },
        }
    }

    fn try_download_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: TrieOrChunkId,
        peer: I,
        partial_chunks: PartialChunks<I>,
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<TrieOrChunk> + From<FetcherRequest<I, TrieOrChunk>>,
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
            .fetch(id, peer)
            .event(move |fetch_result| Event::TrieOrChunkFetched { id, fetch_result })
    }
}

impl<I, REv> Component<REv> for TrieFetcher<I>
where
    REv: ReactorEventT<TrieOrChunk>
        + From<FetcherRequest<I, TrieOrChunk>>
        + From<ControlAnnouncement>
        + From<BlocklistAnnouncement<I>>,
    I: Debug + Clone + Send + Eq + 'static,
{
    type Event = Event<I>;
    type ConstructionError = prometheus::Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Request(TrieFetcherRequest {
                hash,
                responder,
                peers,
            }) => {
                let trie_id = TrieOrChunkId(0, hash);
                let peer = match peers.last() {
                    Some(peer) => peer.clone(),
                    None => {
                        error!(%hash, "tried to fetch trie with no peers available");
                        return Effects::new();
                    }
                };
                let partial_chunks = PartialChunks {
                    responders: vec![responder],
                    peers,
                    chunks: Default::default(),
                    sender: None,
                };
                self.try_download_chunk(effect_builder, trie_id, peer, partial_chunks)
            }
            Event::TrieOrChunkFetched { id, fetch_result } => {
                let hash = id.digest();
                match fetch_result {
                    Err(error) => match self.partial_chunks.remove(hash) {
                        None => {
                            error!(%id,
                                "got a fetch result for a chunk we weren't trying to \
                                    fetch",
                            );
                            Effects::new()
                        }
                        Some(mut partial_chunks) => {
                            warn!(%error, %id, "error fetching trie chunk");
                            // remove the last peer from eligible peers
                            partial_chunks.peers.pop();
                            // try with the next one, if possible
                            match partial_chunks.peers.last().cloned() {
                                Some(next_peer) => self.try_download_chunk(
                                    effect_builder,
                                    id,
                                    next_peer,
                                    partial_chunks,
                                ),
                                None => {
                                    debug!(%id, "couldn't fetch chunk");
                                    partial_chunks.respond(Err(error.into()))
                                }
                            }
                        }
                    },
                    Ok(FetchedData::FromStorage {
                        item: trie_or_chunk,
                    }) => {
                        debug!(%trie_or_chunk, "got trie or chunk from storage");
                        self.consume_trie_or_chunk(effect_builder, None, *trie_or_chunk)
                    }
                    Ok(FetchedData::FromPeer {
                        item: trie_or_chunk,
                        peer,
                    }) => {
                        debug!(?peer, %trie_or_chunk, "got trie or chunk from peer");
                        self.consume_trie_or_chunk(effect_builder, Some(peer), *trie_or_chunk)
                    }
                }
            }
        }
    }
}
