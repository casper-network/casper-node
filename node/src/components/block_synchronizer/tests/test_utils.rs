use std::{collections::BTreeMap, convert::TryInto};

#[cfg(test)]
use casper_hashing::ChunkWithProof;

pub(crate) fn chunks_with_proof_from_data(data: &[u8]) -> BTreeMap<u64, ChunkWithProof> {
    (0..data.chunks(ChunkWithProof::CHUNK_SIZE_BYTES).count())
        .into_iter()
        .map(|index| {
            (
                index as u64,
                ChunkWithProof::new(data, index.try_into().unwrap()).unwrap(),
            )
        })
        .collect()
}
