use std::{
    collections::HashMap,
    fmt::{self, Debug},
};

use datasize::DataSize;
use derive_more::From;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_execution_engine::storage::trie::{Trie, TrieOrChunkedData, TrieOrChunkedDataId};
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
        requests::{FetcherRequest, TrieFetcherRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::Item,
    NodeRng,
};

#[derive(Debug, From, Error)]
pub(crate) enum TrieFetcherError<I>
where
    I: Debug + Eq,
{
    #[error("Fetcher error: {0}")]
    Fetcher(FetcherError<TrieOrChunkedData, I>),
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
    I: Debug + Eq,
{
    peers: Vec<I>,
    responder: Responder<TrieFetcherResult<I>>,
    chunks: HashMap<u64, ChunkWithProof>,
    sender: Option<I>,
}

impl<I> PartialChunks<I>
where
    I: Debug + Eq,
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
}

#[derive(DataSize, Debug)]
pub(crate) struct TrieFetcher<I>
where
    I: Debug + Eq,
{
    partial_chunks: HashMap<Digest, PartialChunks<I>>,
}

#[derive(DataSize, Debug, From)]
pub(crate) enum Event<I>
where
    I: Debug + Eq,
{
    #[from]
    Request(TrieFetcherRequest<I>),
    TrieOrChunkFetched {
        id: TrieOrChunkedDataId,
        fetch_result: FetchResult<TrieOrChunkedData, I>,
    },
}

impl<I> fmt::Display for Event<I>
where
    I: Debug + Eq,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::Request(_) => write!(f, "trie fetcher request"),
            Event::TrieOrChunkFetched { id, .. } => write!(f, "trie or chunk {} fetched", id),
        }
    }
}

fn into_response<I>(trie: Box<Trie<Key, StoredValue>>, sender: Option<I>) -> TrieFetcherResult<I>
where
    I: Debug + Eq,
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
        trie_or_chunk: TrieOrChunkedData,
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<TrieOrChunkedData> + From<FetcherRequest<I, TrieOrChunkedData>>,
    {
        let TrieOrChunkedDataId(_index, hash) = trie_or_chunk.id();
        match trie_or_chunk {
            TrieOrChunkedData::Trie(trie) => match self.partial_chunks.remove(&hash) {
                None => {
                    error!(%hash, "fetched a trie we didn't request!");
                    Effects::new()
                }
                Some(partial_chunks) => {
                    debug!(%hash, "got a full trie");
                    partial_chunks
                        .responder
                        .respond(into_response(trie, sender))
                        .ignore()
                }
            },
            TrieOrChunkedData::ChunkWithProof(chunk) => {
                self.consume_chunk(effect_builder, sender, chunk)
            }
        }
    }

    fn consume_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        sender: Option<I>,
        chunk: ChunkWithProof,
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<TrieOrChunkedData> + From<FetcherRequest<I, TrieOrChunkedData>>,
    {
        if !chunk.verify() {
            match sender {
                None => {
                    error!(?chunk, "got an invalid chunk from storage");
                    return Effects::new();
                }
                Some(sender) => {
                    warn!(?sender, ?chunk, "got an invalid chunk from sender");
                    // TODO: would be good to re-request from someone else instead of the same
                    // node...
                    let id = TrieOrChunkedDataId(chunk.proof().index(), chunk.proof().root_hash());
                    return effect_builder
                        .fetch(id, sender)
                        .event(move |fetch_result| Event::TrieOrChunkFetched { id, fetch_result });
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
                            .responder
                            .respond(Err(TrieFetcherError::Absent(digest, index)))
                            .ignore();
                    }
                };
                let next_id = TrieOrChunkedDataId(missing_index, digest);
                self.try_download_chunk(effect_builder, next_id, peer, partial_chunks)
            }
            None => match partial_chunks.assemble_chunks(count) {
                Ok(trie) => partial_chunks
                    .responder
                    .respond(into_response(Box::new(trie), partial_chunks.sender))
                    .ignore(),
                Err(error) => {
                    error!(%digest, %error,
                        "error while assembling a complete trie",
                    );
                    partial_chunks.responder.respond(Err(error.into())).ignore()
                }
            },
        }
    }

    fn try_download_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: TrieOrChunkedDataId,
        peer: I,
        partial_chunks: PartialChunks<I>,
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<TrieOrChunkedData> + From<FetcherRequest<I, TrieOrChunkedData>>,
    {
        let TrieOrChunkedDataId(_, hash) = id;
        let _ = self.partial_chunks.insert(hash, partial_chunks);
        effect_builder
            .fetch(id, peer)
            .event(move |fetch_result| Event::TrieOrChunkFetched { id, fetch_result })
    }
}

impl<I, REv> Component<REv> for TrieFetcher<I>
where
    REv: ReactorEventT<TrieOrChunkedData> + From<FetcherRequest<I, TrieOrChunkedData>>,
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
                let trie_id = TrieOrChunkedDataId(0, hash);
                let peer = match peers.last() {
                    Some(peer) => peer.clone(),
                    None => {
                        error!(%hash, "tried to fetch trie with no peers available");
                        return Effects::new();
                    }
                };
                let partial_chunks = PartialChunks {
                    responder,
                    peers,
                    chunks: Default::default(),
                    sender: None,
                };
                self.try_download_chunk(effect_builder, trie_id, peer, partial_chunks)
            }
            Event::TrieOrChunkFetched { id, fetch_result } => {
                let TrieOrChunkedDataId(_index, hash) = id;
                match fetch_result {
                    Err(error) => match self.partial_chunks.remove(&hash) {
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
                            let _ = partial_chunks.peers.pop();
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
                                    partial_chunks.responder.respond(Err(error.into())).ignore()
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
