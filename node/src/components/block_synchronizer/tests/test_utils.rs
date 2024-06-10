use std::{collections::BTreeMap, convert::TryInto};

use crate::types::TrieOrChunkId;
#[cfg(test)]
use casper_types::ChunkWithProof;
use rand::Rng;

pub(crate) fn chunks_with_proof_from_data(data: &[u8]) -> BTreeMap<u64, ChunkWithProof> {
    (0..data.chunks(ChunkWithProof::CHUNK_SIZE_BYTES).count())
        .map(|index| {
            (
                index as u64,
                ChunkWithProof::new(data, index.try_into().unwrap()).unwrap(),
            )
        })
        .collect()
}

pub(crate) fn test_chunks_with_proof(
    num_chunks: u64,
) -> (Vec<ChunkWithProof>, Vec<TrieOrChunkId>, Vec<u8>) {
    let mut rng = rand::thread_rng();
    let data: Vec<u8> = (0..ChunkWithProof::CHUNK_SIZE_BYTES * num_chunks as usize)
        .map(|_| rng.gen())
        .collect();

    let chunks = chunks_with_proof_from_data(&data);

    let chunk_ids: Vec<TrieOrChunkId> = chunks
        .iter()
        .map(|(index, chunk)| TrieOrChunkId(*index, chunk.proof().root_hash()))
        .collect();

    (chunks.values().cloned().collect(), chunk_ids, data)
}
