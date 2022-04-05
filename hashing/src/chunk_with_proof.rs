use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, Bytes, FromBytes, ToBytes};

use crate::{
    error::{ChunkWithProofVerificationError, MerkleConstructionError},
    indexed_merkle_proof::IndexedMerkleProof,
    Digest,
};

/// Represents a chunk of data with attached proof.
#[derive(DataSize, PartialEq, Eq, Debug, Clone, JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkWithProof {
    proof: IndexedMerkleProof,
    #[schemars(with = "String", description = "Hex-encoded bytes.")]
    chunk: Bytes,
}

impl ToBytes for ChunkWithProof {
    fn write_bytes(&self, buf: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        buf.append(&mut self.proof.to_bytes()?);
        buf.append(&mut self.chunk.to_bytes()?);

        Ok(())
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.proof.serialized_length() + self.chunk.serialized_length()
    }
}

impl FromBytes for ChunkWithProof {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (proof, remainder) = FromBytes::from_bytes(bytes)?;
        let (chunk, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((ChunkWithProof { proof, chunk }, remainder))
    }
}

impl ChunkWithProof {
    #[cfg(test)]
    /// 10 bytes for testing purposes.
    pub const CHUNK_SIZE_BYTES: usize = 10;

    #[cfg(not(test))]
    /// 1 MiB
    pub const CHUNK_SIZE_BYTES: usize = 1 << 20;

    /// Constructs the [`ChunkWithProof`] that contains the chunk of data with the appropriate index
    /// and the cryptographic proof.
    ///
    /// Empty data is always represented as single, empty chunk and not as zero chunks.
    pub fn new(data: &[u8], index: u64) -> Result<Self, MerkleConstructionError> {
        Ok(if data.is_empty() {
            ChunkWithProof {
                proof: IndexedMerkleProof::new([Digest::blake2b_hash(&[])], index)?,
                chunk: Bytes::new(),
            }
        } else {
            ChunkWithProof {
                proof: IndexedMerkleProof::new(
                    data.chunks(Self::CHUNK_SIZE_BYTES)
                        .map(Digest::blake2b_hash),
                    index,
                )?,
                chunk: Bytes::from(
                    data.chunks(Self::CHUNK_SIZE_BYTES)
                        .nth(index as usize)
                        .ok_or_else(|| MerkleConstructionError::IndexOutOfBounds {
                            count: data.chunks(Self::CHUNK_SIZE_BYTES).len() as u64,
                            index,
                        })?,
                ),
            }
        })
    }

    /// Get a reference to the `ChunkWithProof`'s chunk.
    pub fn chunk(&self) -> &[u8] {
        self.chunk.as_slice()
    }

    /// Convert a chunk with proof into the underlying chunk.
    pub fn into_chunk(self) -> Bytes {
        self.chunk
    }

    /// Returns the `IndexedMerkleProof`.
    pub fn proof(&self) -> &IndexedMerkleProof {
        &self.proof
    }

    /// Verify the integrity of this chunk with indexed Merkle proof.
    pub fn verify(&self) -> Result<(), ChunkWithProofVerificationError> {
        let _ = self.proof().verify()?;
        let first_digest_in_indexed_merkle_proof =
            self.proof().merkle_proof().first().ok_or_else(|| {
                ChunkWithProofVerificationError::ChunkWithProofHasEmptyMerkleProof {
                    chunk_with_proof: self.clone(),
                }
            })?;
        let hash_of_chunk = Digest::hash(self.chunk());
        if *first_digest_in_indexed_merkle_proof != hash_of_chunk {
            return Err(
                ChunkWithProofVerificationError::FirstDigestInMerkleProofDidNotMatchHashOfChunk {
                    first_digest_in_indexed_merkle_proof: *first_digest_in_indexed_merkle_proof,
                    hash_of_chunk,
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };
    use proptest_attr_macro::proptest;
    use rand::Rng;

    use casper_types::bytesrepr::{self, FromBytes, ToBytes};

    use crate::{chunk_with_proof::ChunkWithProof, error::MerkleConstructionError, Digest};

    fn prepare_bytes(length: usize) -> Vec<u8> {
        let mut rng = rand::thread_rng();

        (0..length).into_iter().map(|_| rng.gen()).collect()
    }

    fn random_chunk_with_proof() -> ChunkWithProof {
        let mut rng = rand::thread_rng();
        let data: Vec<u8> = prepare_bytes(rng.gen_range(1..1024));
        let index = rng.gen_range(0..data.chunks(ChunkWithProof::CHUNK_SIZE_BYTES).len() as u64);

        ChunkWithProof::new(&data, index).unwrap()
    }

    impl ChunkWithProof {
        fn replace_first_proof(self) -> Self {
            let mut rng = rand::thread_rng();
            let ChunkWithProof { mut proof, chunk } = self;

            // Keep the same number of proofs, but replace the first one with some random hash
            let mut merkle_proof: Vec<_> = proof.merkle_proof().to_vec();
            merkle_proof.pop();
            merkle_proof.insert(0, Digest::hash(rng.gen::<usize>().to_string()));
            proof.inject_merkle_proof(merkle_proof);

            ChunkWithProof { proof, chunk }
        }
    }

    #[derive(Debug)]
    pub struct TestDataSize(usize);
    impl Arbitrary for TestDataSize {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            (0usize..32usize)
                .prop_map(|chunk_count| {
                    TestDataSize(chunk_count * ChunkWithProof::CHUNK_SIZE_BYTES)
                })
                .boxed()
        }
    }

    #[derive(Debug)]
    pub struct TestDataSizeAtLeastTwoChunks(usize);
    impl Arbitrary for TestDataSizeAtLeastTwoChunks {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_: Self::Parameters) -> Self::Strategy {
            (2usize..32usize)
                .prop_map(|chunk_count| {
                    TestDataSizeAtLeastTwoChunks(chunk_count * ChunkWithProof::CHUNK_SIZE_BYTES)
                })
                .boxed()
        }
    }

    #[proptest]
    fn generates_valid_proof(test_data: TestDataSize) {
        for data in [prepare_bytes(test_data.0), vec![0u8; test_data.0]] {
            let number_of_chunks: u64 = data
                .chunks(ChunkWithProof::CHUNK_SIZE_BYTES)
                .len()
                .try_into()
                .unwrap();

            assert!((0..number_of_chunks)
                .into_iter()
                .map(|chunk_index| { ChunkWithProof::new(data.as_slice(), chunk_index).unwrap() })
                .all(|chunk_with_proof| chunk_with_proof.verify().is_ok()));
        }
    }

    #[proptest]
    fn validate_chunks_against_hash_merkle_tree(test_data: TestDataSizeAtLeastTwoChunks) {
        // This test requires at least two chunks
        assert!(test_data.0 >= ChunkWithProof::CHUNK_SIZE_BYTES * 2);

        for data in [prepare_bytes(test_data.0), vec![0u8; test_data.0]] {
            let expected_root = Digest::hash_merkle_tree(
                data.chunks(ChunkWithProof::CHUNK_SIZE_BYTES)
                    .map(Digest::hash),
            );

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

    #[proptest]
    fn verifies_chunk_with_proofs(test_data: TestDataSize) {
        for data in [prepare_bytes(test_data.0), vec![0u8; test_data.0]] {
            let chunk_with_proof = ChunkWithProof::new(data.as_slice(), 0).unwrap();
            assert!(chunk_with_proof.verify().is_ok());

            let chunk_with_incorrect_proof = chunk_with_proof.replace_first_proof();
            assert!(chunk_with_incorrect_proof.verify().is_err());
        }
    }

    #[proptest]
    fn serde_deserialization_of_malformed_chunk_should_work(test_data: TestDataSize) {
        for data in [prepare_bytes(test_data.0), vec![0u8; test_data.0]] {
            let chunk_with_proof = ChunkWithProof::new(data.as_slice(), 0).unwrap();

            let json = serde_json::to_string(&chunk_with_proof).unwrap();
            assert_eq!(
                chunk_with_proof,
                serde_json::from_str::<ChunkWithProof>(&json)
                    .expect("should deserialize correctly")
            );

            let chunk_with_incorrect_proof = chunk_with_proof.replace_first_proof();
            let json = serde_json::to_string(&chunk_with_incorrect_proof).unwrap();
            serde_json::from_str::<ChunkWithProof>(&json).expect("should deserialize correctly");
        }
    }

    #[proptest]
    fn bytesrepr_deserialization_of_malformed_chunk_should_work(test_data: TestDataSize) {
        for data in [prepare_bytes(test_data.0), vec![0u8; test_data.0]] {
            let chunk_with_proof = ChunkWithProof::new(data.as_slice(), 0).unwrap();

            let bytes = chunk_with_proof
                .to_bytes()
                .expect("should serialize correctly");

            let (deserialized_chunk_with_proof, _) =
                ChunkWithProof::from_bytes(&bytes).expect("should deserialize correctly");

            assert_eq!(chunk_with_proof, deserialized_chunk_with_proof);

            let chunk_with_incorrect_proof = chunk_with_proof.replace_first_proof();
            let bytes = chunk_with_incorrect_proof
                .to_bytes()
                .expect("should serialize correctly");

            ChunkWithProof::from_bytes(&bytes).expect("should deserialize correctly");
        }
    }

    #[test]
    fn returns_error_on_incorrect_index() {
        // This test needs specific data sizes, hence it doesn't use the proptest

        let chunk_with_proof = ChunkWithProof::new(&[], 0).expect("should create with empty data");
        assert!(chunk_with_proof.verify().is_ok());

        let chunk_with_proof =
            ChunkWithProof::new(&[], 1).expect_err("should error with empty data and index > 0");
        if let MerkleConstructionError::IndexOutOfBounds { count, index } = chunk_with_proof {
            assert_eq!(count, 1);
            assert_eq!(index, 1);
        } else {
            panic!("expected MerkleConstructionError::IndexOutOfBounds");
        }

        let data_larger_than_single_chunk = vec![0u8; ChunkWithProof::CHUNK_SIZE_BYTES * 10];
        ChunkWithProof::new(data_larger_than_single_chunk.as_slice(), 9).unwrap();

        let chunk_with_proof =
            ChunkWithProof::new(data_larger_than_single_chunk.as_slice(), 10).unwrap_err();
        if let MerkleConstructionError::IndexOutOfBounds { count, index } = chunk_with_proof {
            assert_eq!(count, 10);
            assert_eq!(index, 10);
        } else {
            panic!("expected MerkleConstructionError::IndexOutOfBounds");
        }
    }

    #[test]
    fn bytesrepr_serialization() {
        let chunk_with_proof = random_chunk_with_proof();
        bytesrepr::test_serialization_roundtrip(&chunk_with_proof);
    }

    #[test]
    fn chunk_with_empty_data_contains_a_single_proof() {
        let chunk_with_proof = ChunkWithProof::new(&[], 0).unwrap();
        assert_eq!(chunk_with_proof.proof.merkle_proof().len(), 1)
    }
}
