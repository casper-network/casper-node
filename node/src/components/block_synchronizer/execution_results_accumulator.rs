use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug},
};

use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, warn};

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::bytesrepr::{self};

use crate::{
    components::{
        fetcher::{Error as FetcherError, FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::PeerBehaviorAnnouncement,
        requests::{ExecutionResultsAccumulatorRequest, FetcherRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
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
    #[error("no peers provided in request to fetch execution results")]
    NoPeersProvided,
    #[error("fetched execution results for {provided} passed to accumulator for {expected}")]
    BlockHashMismatch {
        expected: BlockHash,
        provided: BlockHash,
    },
}

pub(crate) enum Outcome {
    Need {
        id: BlockEffectsOrChunkId,
    },
    Got {
        value: Result<Vec<casper_types::ExecutionResult>, Error>,
    },
}

#[derive(DataSize, Debug)]
pub(super) struct ExecutionResultsAccumulator {
    block_hash: BlockHash,
    is_legacy: bool,
    results_root_hash: Option<Digest>,
    chunks: HashMap<u64, ChunkWithProof>,
}

impl ExecutionResultsAccumulator {
    pub(crate) fn new(
        block_hash: BlockHash,
        results_root_hash: Option<Digest>,
    ) -> Result<(Self, BlockEffectsOrChunkId), Error> {
        let mut accumulator = ExecutionResultsAccumulator {
            block_hash,
            is_legacy: results_root_hash.is_none(),
            results_root_hash,
            chunks: HashMap::new(),
        };
        let id = if accumulator.is_legacy {
            BlockEffectsOrChunkId::legacy(block_hash)
        } else {
            BlockEffectsOrChunkId::new(block_hash)
        };

        Ok((accumulator, id))
    }

    pub(super) fn handle_fetch_result(
        &mut self,
        id: BlockEffectsOrChunkId,
        fetch_result: FetchResult<BlockEffectsOrChunk>,
    ) -> Outcome {
        match fetch_result {
            Ok(FetchedData::FromStorage {
                item: block_effects_or_chunk,
            })
            | Ok(FetchedData::FromPeer {
                item: block_effects_or_chunk,
                ..
            }) => self.consume_block_effects_or_chunk(*block_effects_or_chunk),
            Err(error) => {
                warn!(%error, %id, "error fetching execution results chunk");
                Outcome::Need { id }
            }
        }
    }

    fn consume_block_effects_or_chunk(
        &mut self,
        block_effects_or_chunk: BlockEffectsOrChunk,
    ) -> Outcome {
        debug!(%block_effects_or_chunk, "got block effects or chunk");
        let id = block_effects_or_chunk.id();
        let (block_hash, value) = match block_effects_or_chunk {
            BlockEffectsOrChunk::BlockEffectsLegacy { block_hash, value } => (block_hash, value),
            BlockEffectsOrChunk::BlockEffects { block_hash, value } => (block_hash, value),
        };

        if self.block_hash != block_hash {
            return Outcome::Got {
                value: Err(Error::BlockHashMismatch {
                    expected: self.block_hash,
                    provided: block_hash,
                }),
            };
        }

        match value {
            ValueOrChunk::Value(execution_results) => {
                debug!(%block_hash, "got a full set of execution results (unchunked)");
                Outcome::Got {
                    value: Ok(execution_results),
                }
            }
            ValueOrChunk::ChunkWithProof(chunk) => self.consume_chunk(id, chunk),
        }
    }

    fn consume_chunk(&mut self, id: BlockEffectsOrChunkId, chunk: ChunkWithProof) -> Outcome {
        let digest = chunk.proof().root_hash();
        let index = chunk.proof().index();
        let count = chunk.proof().count();

        // Add the downloaded chunk to cache.
        match self.results_root_hash {
            None => {
                // TODO - we just accept the first chunk's digest - could probably do better.
                self.results_root_hash = Some(digest);
            }
            Some(root_hash) => {
                if root_hash != digest {
                    debug!(%root_hash, ?chunk, "chunk with different root hash");
                    return Outcome::Need { id };
                }
            }
        }
        let _ = self.chunks.insert(index, chunk);

        // Check if we can now return a complete value.
        match self.missing_chunk(count) {
            Some(missing_index) => {
                let next_id = id.next_chunk(missing_index);
                Outcome::Need { id: next_id }
            }
            None => {
                debug!(%self.block_hash, "got a full set of execution results (chunked)");
                let value = self.assemble_chunks(count);
                Outcome::Got { value }
            }
        }
    }

    fn missing_chunk(&self, count: u64) -> Option<u64> {
        (0..count).find(|idx| !self.chunks.contains_key(idx))
    }

    fn assemble_chunks(&self, count: u64) -> Result<Vec<casper_types::ExecutionResult>, Error> {
        let serialized: Vec<u8> = (0..count)
            .filter_map(|index| self.chunks.get(&index))
            .flat_map(|chunk| chunk.chunk())
            .copied()
            .collect();
        bytesrepr::deserialize(serialized).map_err(|error| {
            error!(%error, "failed to deserialize execution results");
            Error::Bytesrepr(error)
        })
    }
}
