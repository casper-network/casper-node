use std::convert::TryFrom;

use crate::{
    error::{self},
    indexed_merkle_proof::IndexedMerkleProof,
    util::blake2b_hash,
};

#[cfg_attr(
    feature = "std",
    derive(serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct ChunkWithProofDeserializeValidator {
    proof: IndexedMerkleProof,
    chunk: Vec<u8>,
}

impl TryFrom<ChunkWithProofDeserializeValidator> for ChunkWithProof {
    type Error = error::MerkleConstructionError;
    fn try_from(value: ChunkWithProofDeserializeValidator) -> Result<Self, Self::Error> {
        let candidate = Self {
            proof: value.proof,
            chunk: value.chunk,
        };
        if candidate.is_valid() {
            Ok(candidate)
        } else {
            Err(error::MerkleConstructionError::IncorrectChunkProof)
        }
    }
}

#[cfg_attr(
    feature = "std",
    derive(
        PartialEq,
        Debug,
        schemars::JsonSchema,
        serde::Serialize,
        serde::Deserialize
    ),
    schemars(with = "String", description = "Hex-encoded hash digest."),
    serde(deny_unknown_fields, try_from = "ChunkWithProofDeserializeValidator")
)]
pub struct ChunkWithProof {
    proof: IndexedMerkleProof,
    chunk: Vec<u8>,
}

impl ChunkWithProof {
    #[cfg(test)]
    pub const CHUNK_SIZE: usize = 1;

    #[cfg(not(test))]
    pub const CHUNK_SIZE: usize = 1_048_576; // 2^20

    pub fn new(data: &[u8], index: u64) -> Result<Self, error::MerkleConstructionError> {
        if data.len() < Self::CHUNK_SIZE * (index as usize) {
            return Err(error::MerkleConstructionError::IndexOutOfBounds {
                count: data.chunks(Self::CHUNK_SIZE).len() as u64,
                index,
            });
        }

        let (proof, chunk) = if data.is_empty() {
            (IndexedMerkleProof::new([blake2b_hash(&[])], index)?, vec![])
        } else {
            (
                IndexedMerkleProof::new(data.chunks(Self::CHUNK_SIZE).map(blake2b_hash), index)?,
                data[Self::CHUNK_SIZE * (index as usize)
                    ..data.len().min(Self::CHUNK_SIZE * ((index as usize) + 1))]
                    .to_vec(),
            )
        };

        Ok(ChunkWithProof { proof, chunk })
    }

    fn is_valid(&self) -> bool {
        let chunk_hash = blake2b_hash(self.chunk());
        self.proof
            .merkle_proof()
            .first()
            .map_or(false, |first_hash| chunk_hash == *first_hash)
    }

    /// Get a reference to the chunk with proof's chunk.
    pub fn chunk(&self) -> &[u8] {
        self.chunk.as_slice()
    }
}

#[cfg(test)]
mod test {
    use crate::error;
    use proptest::proptest;
    use rand::Rng;
    use std::{convert::TryInto, ops::Range};

    use crate::{
        chunk_with_proof::ChunkWithProof,
        util::{blake2b_hash, hash_merkle_tree},
    };

    fn testing_data_size_range(minimum_size: usize) -> Range<usize> {
        minimum_size..512usize
    }

    proptest! {
        #[test]
        fn generates_valid_proof(data_size in testing_data_size_range(0)) {
            let data = vec![0u8; ChunkWithProof::CHUNK_SIZE * data_size];

            let number_of_chunks: u64 = data
                .chunks(ChunkWithProof::CHUNK_SIZE)
                .len()
                .try_into()
                .unwrap();

            assert!(!(0..number_of_chunks)
                .into_iter()
                .map(|chunk_index| {
                    ChunkWithProof::new(data.as_slice(), chunk_index).unwrap()
                })
                .map(|chunk_with_proof| chunk_with_proof.is_valid())
                .any(|valid| !valid));
            }
    }

    proptest! {
        #[test]
        fn validate_chunks_against_hash_merkle_tree(data_size in testing_data_size_range(ChunkWithProof::CHUNK_SIZE * 2)) {
            // This test requires at least two chunks to be present, hence `testing_data_size_range(ChunkWithProof::CHUNK_SIZE * 2)`

            let data = vec![0u8; ChunkWithProof::CHUNK_SIZE * data_size];
            let expected_root =
                hash_merkle_tree(data.chunks(ChunkWithProof::CHUNK_SIZE).map(blake2b_hash));

            // Calculate proof with `ChunkWithProof`
            let ChunkWithProof {
                proof: proof_0,
                chunk: _,
            } = ChunkWithProof::new(data.as_slice(), 0).unwrap();
            let ChunkWithProof {
                proof: proof_1,
                chunk: _,
            } = ChunkWithProof::new(data.as_slice(), 1).unwrap();

            assert_eq!(proof_0.root_hash(), expected_root);
            assert_eq!(proof_1.root_hash(), expected_root);
        }
    }

    proptest! {
        #[test]
        fn validates_chunk_with_proofs(data_size in testing_data_size_range(0)) {
            let data = vec![0u8; ChunkWithProof::CHUNK_SIZE * data_size];

            impl ChunkWithProof {
                fn replace_first_proof(self) -> Self {
                    let mut rng = rand::thread_rng();
                    let ChunkWithProof { mut proof, chunk } = self;

                    // Keep the same number of proofs, but replace the first one with some random hash
                    let mut merkle_proof: Vec<_> = proof.merkle_proof().to_vec();
                    merkle_proof.pop();
                    merkle_proof.insert(0, blake2b_hash(rng.gen::<usize>().to_string()));
                    proof.inject_merkle_proof(merkle_proof);

                    ChunkWithProof { proof, chunk }
                }
            }

            let chunk_with_proof =
                ChunkWithProof::new(data.as_slice(), 0).unwrap();
            assert!(chunk_with_proof.is_valid());

            let chunk_with_incorrect_proof = chunk_with_proof.replace_first_proof();
            assert!(!chunk_with_incorrect_proof.is_valid());
        }
    }

    proptest! {
        #[test]
        fn validates_chunk_with_proof_after_deserialization(data_size in testing_data_size_range(0)) {
            let data = vec![0u8; ChunkWithProof::CHUNK_SIZE * data_size];
            let chunk_with_proof =
                ChunkWithProof::new(data.as_slice(), 0).unwrap();

            let json = serde_json::to_string(&chunk_with_proof).unwrap();
            assert_eq!(
                chunk_with_proof,
                serde_json::from_str::<ChunkWithProof>(&json).expect("should deserialize correctly")
            );

            let chunk_with_incorrect_proof = chunk_with_proof.replace_first_proof();
            let json = serde_json::to_string(&chunk_with_incorrect_proof).unwrap();
            serde_json::from_str::<ChunkWithProof>(&json).expect_err("shoud not deserialize correctly");
        }
    }

    #[test]
    fn returns_error_on_incorrect_index() {
        // This test needs specific data sizes, hence it doesn't use the proptest

        let chunk_with_proof = ChunkWithProof::new(&[], 0).expect("should create with empty data");
        assert!(chunk_with_proof.is_valid());

        let chunk_with_proof =
            ChunkWithProof::new(&[], 1).expect_err("should error with empty data and index > 0");
        if let error::MerkleConstructionError::IndexOutOfBounds { count, index } = chunk_with_proof
        {
            assert_eq!(count, 0);
            assert_eq!(index, 1);
        } else {
            panic!("expected MerkleConstructionError::IndexOutOfBounds");
        }

        let data_larger_than_single_chunk = vec![0u8; ChunkWithProof::CHUNK_SIZE * 10];
        ChunkWithProof::new(data_larger_than_single_chunk.as_slice(), 9).unwrap();

        let chunk_with_proof =
            ChunkWithProof::new(data_larger_than_single_chunk.as_slice(), 10).unwrap_err();
        if let error::MerkleConstructionError::IndexOutOfBounds { count, index } = chunk_with_proof
        {
            assert_eq!(count, 10);
            assert_eq!(index, 10);
        } else {
            panic!("expected MerkleConstructionError::IndexOutOfBounds");
        }
    }
}
