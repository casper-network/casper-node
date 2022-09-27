use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug},
    hash::Hash,
};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_execution_engine::storage::trie::TrieRaw;
use casper_hashing::{ChunkWithProof, Digest};
use casper_types::{
    bytesrepr::{self, Bytes},
    Key, StoredValue,
};

use crate::{
    components::{
        fetcher::{Error as FetcherError, FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::{ControlAnnouncement, PeerBehaviorAnnouncement},
        requests::{ExecutionResultsAccumulatorRequest, FetcherRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::{BlockEffectsOrChunk, BlockEffectsOrChunkId, BlockHash, Item, NodeId, ValueOrChunk},
    NodeRng,
};

#[derive(Debug, From, Error, Clone, Serialize)]
pub(crate) enum Error {
    #[error("execution results or chunk fetcher error: {0}")]
    Fetcher(FetcherError<BlockEffectsOrChunk>),
    #[error("execution results or chunk serialization error: {0}")]
    Bytesrepr(bytesrepr::Error),
    #[error("couldn't fetch execution results chunk {0}")]
    Absent(BlockEffectsOrChunkId),
}

#[derive(DataSize, Debug)]
struct PartialChunks {
    results_root_hash: Option<Digest>,
    peers: Vec<NodeId>,
    responders: Vec<Responder<Result<Vec<casper_types::ExecutionResult>, Error>>>,
    chunks: HashMap<u64, ChunkWithProof>,
}

impl PartialChunks {
    fn missing_chunk(&self, count: u64) -> Option<u64> {
        (0..count).find(|idx| !self.chunks.contains_key(idx))
    }

    fn assemble_chunks(&self, count: u64) -> Result<Vec<casper_types::ExecutionResult>, Error> {
        let serialized: Vec<u8> = (0..count)
            .filter_map(|index| self.chunks.get(&index))
            .flat_map(|chunk| chunk.chunk())
            .copied()
            .collect();
        bytesrepr::deserialize(serialized).map_err(Error::Bytesrepr)
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

    fn respond(self, value: Result<Vec<casper_types::ExecutionResult>, Error>) -> Effects<Event> {
        self.responders
            .into_iter()
            .flat_map(|responder| responder.respond(value.clone()).ignore())
            .collect()
    }
}

#[derive(DataSize, Debug, From, Serialize)]
pub(crate) enum Event {
    #[from]
    Request(ExecutionResultsAccumulatorRequest),
    BlockEffectsOrChunkFetched {
        id: BlockEffectsOrChunkId,
        fetch_result: FetchResult<BlockEffectsOrChunk>,
    },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Event::Request(_) => write!(f, "block effects fetcher request"),
            Event::BlockEffectsOrChunkFetched { id, .. } => {
                write!(f, "got a result for block effects or chunk {}", id)
            }
        }
    }
}

#[derive(DataSize, Debug)]
pub(super) struct ExecutionResultsAccumulator {
    partial_chunks: HashMap<BlockHash, PartialChunks>,
}

impl ExecutionResultsAccumulator {
    pub(crate) fn new() -> Self {
        ExecutionResultsAccumulator {
            partial_chunks: Default::default(),
        }
    }

    fn consume_block_effects_or_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        block_effects_or_chunk: BlockEffectsOrChunk,
    ) -> Option<Effects<Event>>
    where
        REv: From<FetcherRequest<BlockEffectsOrChunk>> + From<PeerBehaviorAnnouncement> + Send,
    {
        let id = block_effects_or_chunk.id();
        let (block_hash, value) = match block_effects_or_chunk {
            BlockEffectsOrChunk::BlockEffectsLegacy { block_hash, value } => (block_hash, value),
            BlockEffectsOrChunk::BlockEffects { block_hash, value } => (block_hash, value),
        };

        match value {
            ValueOrChunk::Value(execution_results) => match self.partial_chunks.remove(&block_hash)
            {
                None => {
                    error!(%block_hash, "fetched execution results we didn't request!");
                    Some(Effects::new())
                }
                Some(partial_chunks) => {
                    debug!(%block_hash, "got a full set of execution results");
                    Some(partial_chunks.respond(Ok(execution_results)))
                }
            },
            ValueOrChunk::ChunkWithProof(chunk) => self.consume_chunk(effect_builder, id, chunk),
        }
    }

    fn consume_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: BlockEffectsOrChunkId,
        chunk: ChunkWithProof,
    ) -> Option<Effects<Event>>
    where
        REv: From<FetcherRequest<BlockEffectsOrChunk>> + From<PeerBehaviorAnnouncement> + Send,
    {
        let digest = chunk.proof().root_hash();
        let index = chunk.proof().index();
        let count = chunk.proof().count();
        let mut partial_chunks = match self.partial_chunks.remove(id.block_hash()) {
            None => {
                error!(%digest, %index, "got a chunk that wasn't requested");
                return Some(Effects::new());
            }
            Some(partial_chunks) => partial_chunks,
        };

        // Add the downloaded chunk to cache.

        match partial_chunks.results_root_hash {
            None => {
                // TODO - we just accept the first chunk's digest - could probably do better.
                partial_chunks.results_root_hash = Some(digest);
            }
            Some(root_hash) => {
                if root_hash != digest {
                    debug!(%root_hash, ?chunk, "chunk with different root hash");
                    // Return `None` to cause the chunk to be re-requested.
                    return None;
                }
            }
        }
        let _ = partial_chunks.chunks.insert(index, chunk);

        // Check if we can now return a complete value.
        match partial_chunks.missing_chunk(count) {
            Some(missing_index) => {
                let peer = match partial_chunks.peers.last() {
                    Some(peer) => *peer,
                    None => {
                        debug!(
                            %digest, %missing_index,
                            "no peers to download the next chunk from, giving up",
                        );
                        return Some(partial_chunks.respond(Err(Error::Absent(id))));
                    }
                };
                let next_id = id.next_chunk(missing_index);
                Some(self.try_download_chunk(effect_builder, next_id, peer, partial_chunks))
            }
            None => {
                let value = match partial_chunks.assemble_chunks(count) {
                    Ok(value) => value,
                    Err(error) => {
                        error!(%error, "failed to deserialize execution results");
                        return Some(partial_chunks.respond(Err(error)));
                    }
                };
                Some(partial_chunks.respond(Ok(value)))
            }
        }
    }

    fn try_download_chunk<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        id: BlockEffectsOrChunkId,
        peer: NodeId,
        partial_chunks: PartialChunks,
    ) -> Effects<Event>
    where
        REv: From<FetcherRequest<BlockEffectsOrChunk>> + Send,
    {
        let maybe_old_partial_chunks = self.partial_chunks.insert(*id.block_hash(), partial_chunks);
        if let Some(old_partial_chunks) = maybe_old_partial_chunks {
            // unwrap is safe as we just inserted a value at this key
            self.partial_chunks
                .get_mut(id.block_hash())
                .unwrap()
                .merge(old_partial_chunks);
        }
        effect_builder
            .fetch::<BlockEffectsOrChunk>(id, peer, ())
            .event(move |fetch_result| Event::BlockEffectsOrChunkFetched { id, fetch_result })
    }
}

impl<REv> Component<REv> for ExecutionResultsAccumulator
where
    REv: From<FetcherRequest<BlockEffectsOrChunk>> + From<PeerBehaviorAnnouncement> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        debug!(?event, "handling event");
        match event {
            Event::Request(ExecutionResultsAccumulatorRequest {
                block_hash,
                results_root_hash,
                peers,
                responder,
            }) => {
                let id = match results_root_hash {
                    Some(_) => BlockEffectsOrChunkId::new(block_hash),
                    None => BlockEffectsOrChunkId::legacy(block_hash),
                };
                // TODO - use more than one peer at a time?
                let peer = match peers.last() {
                    Some(peer) => *peer,
                    None => {
                        error!(
                            %block_hash,
                            "tried to fetch execution results with no peers available"
                        );
                        return Effects::new();
                    }
                };
                let partial_chunks = PartialChunks {
                    results_root_hash,
                    responders: vec![responder],
                    peers,
                    chunks: Default::default(),
                };
                self.try_download_chunk(effect_builder, id, peer, partial_chunks)
            }
            Event::BlockEffectsOrChunkFetched { id, fetch_result } => {
                let block_hash = id.block_hash();
                match fetch_result {
                    Err(error) => match self.partial_chunks.remove(block_hash) {
                        None => {
                            error!(
                                %id,
                                "got a fetch result for a chunk we weren't trying to fetch",
                            );
                            Effects::new()
                        }
                        Some(mut partial_chunks) => {
                            warn!(%error, %id, "error fetching trie chunk");
                            // try with the next peer, if possible
                            match partial_chunks.next_peer().cloned() {
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
                        item: block_effects_or_chunk,
                    })
                    | Ok(FetchedData::FromPeer {
                        item: block_effects_or_chunk,
                        ..
                    }) => {
                        debug!(%block_effects_or_chunk, "got block effects or chunk");
                        let id = block_effects_or_chunk.id();
                        match self
                            .consume_block_effects_or_chunk(effect_builder, *block_effects_or_chunk)
                        {
                            None => {
                                // The chunk root hash didn't match the other(s) - try next peer.
                                match self.partial_chunks.remove(block_hash) {
                                    None => {
                                        error!(
                                            %id,
                                            "got a fetch result for a chunk we weren't trying to fetch",
                                        );
                                        Effects::new()
                                    }
                                    Some(mut partial_chunks) => {
                                        // try with the next peer, if possible
                                        match partial_chunks.next_peer().cloned() {
                                            Some(next_peer) => self.try_download_chunk(
                                                effect_builder,
                                                id,
                                                next_peer,
                                                partial_chunks,
                                            ),
                                            None => {
                                                debug!(%id, "couldn't fetch chunk");
                                                partial_chunks.respond(Err(Error::Absent(id)))
                                            }
                                        }
                                    }
                                }
                            }
                            Some(effects) => effects,
                        }
                    }
                }
            }
        }
    }
}
