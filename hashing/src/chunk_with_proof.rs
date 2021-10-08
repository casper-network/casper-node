use crate::indexed_merkle_proof::IndexedMerkleProof;

use casper_types::{
    allocate_buffer,
    bytesrepr::{Bytes, FromBytes, ToBytes},
};

use crate::{error, Digest};

/// Represents the chunk of data with attached [IndexedMerkleProof].
/// Empty data is always represented as single, empty chunk and not as zero chunks.
#[derive(PartialEq, Debug, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[schemars(with = "String", description = "Hex-encoded hash digest.")]
#[serde(deny_unknown_fields)]
pub struct ChunkWithProof {
    proof: IndexedMerkleProof,
    chunk: Vec<u8>,
}

impl ToBytes for ChunkWithProof {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.proof.to_bytes()?);
        result.append(&mut Bytes::from(self.chunk.as_slice()).to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.proof.serialized_length() + self.chunk.serialized_length()
    }
}

impl FromBytes for ChunkWithProof {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let ((proof, chunk), remainder) = <(IndexedMerkleProof, Bytes)>::from_bytes(bytes)?;
        Ok((
            ChunkWithProof {
                proof,
                chunk: chunk.into(),
            },
            remainder,
        ))
    }
}

impl ChunkWithProof {
    #[cfg(test)]
    pub const CHUNK_SIZE_BYTES: usize = 10;

    #[cfg(not(test))]
    pub const CHUNK_SIZE_BYTES: usize = 1_048_576; // 2^20

    #[allow(unused)]
    fn new(data: &[u8], index: u64) -> Result<Self, error::MerkleConstructionError> {
        if data.len() < Self::CHUNK_SIZE_BYTES * (index as usize) {
            return Err(error::MerkleConstructionError::IndexOutOfBounds {
                count: data.chunks(Self::CHUNK_SIZE_BYTES).len() as u64,
                index,
            });
        }

        let (proof, chunk) = if data.is_empty() {
            (IndexedMerkleProof::new([Digest::hash(&[])], index)?, vec![])
        } else {
            (
                IndexedMerkleProof::new(
                    data.chunks(Self::CHUNK_SIZE_BYTES).map(Digest::hash),
                    index,
                )?,
                data[Self::CHUNK_SIZE_BYTES * (index as usize)
                    ..data
                        .len()
                        .min(Self::CHUNK_SIZE_BYTES * ((index as usize) + 1))]
                    .to_vec(),
            )
        };

        Ok(ChunkWithProof { proof, chunk })
    }

    /// Get a reference to the chunk with proof's chunk.
    fn chunk(&self) -> &[u8] {
        self.chunk.as_slice()
    }

    #[allow(unused)]
    pub(crate) fn verify(&self) -> bool {
        let chunk_hash = Digest::hash(self.chunk());
        self.proof
            .merkle_proof()
            .first()
            .map_or(false, |first_hash| chunk_hash == *first_hash)
    }
}

#[cfg(test)]
mod test {
    use crate::{error, Digest};
    use casper_types::bytesrepr::{FromBytes, ToBytes};
    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };
    use proptest_attr_macro::proptest;
    use rand::Rng;
    use std::convert::TryInto;

    use crate::chunk_with_proof::ChunkWithProof;

    fn prepare_bytes(length: usize) -> Vec<u8> {
        let mut rng = rand::thread_rng();

        let mut v = Vec::with_capacity(length);
        (0..length).into_iter().for_each(|_| v.push(rng.gen()));
        v
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
                .all(|chunk_with_proof| chunk_with_proof.verify()));
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
            assert!(chunk_with_proof.verify());

            let chunk_with_incorrect_proof = chunk_with_proof.replace_first_proof();
            assert!(!chunk_with_incorrect_proof.verify());
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
        assert!(chunk_with_proof.verify());

        let chunk_with_proof =
            ChunkWithProof::new(&[], 1).expect_err("should error with empty data and index > 0");
        if let error::MerkleConstructionError::IndexOutOfBounds { count, index } = chunk_with_proof
        {
            assert_eq!(count, 0);
            assert_eq!(index, 1);
        } else {
            panic!("expected MerkleConstructionError::IndexOutOfBounds");
        }

        let data_larger_than_single_chunk = vec![0u8; ChunkWithProof::CHUNK_SIZE_BYTES * 10];
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

    #[test]
    fn bytesrepr_serialization() {
        let original_chunk_with_proof = ChunkWithProof::new(b"ABCDEF123456", 1).unwrap();

        let bytes = original_chunk_with_proof
            .to_bytes()
            .expect("should serialize correctly");

        let (deserialized_chunk_with_proof, remainder) =
            ChunkWithProof::from_bytes(&bytes).expect("should deserialize correctly");

        assert_eq!(original_chunk_with_proof, deserialized_chunk_with_proof);
        assert!(remainder.is_empty());
    }

    #[test]
    fn bytesrepr_serialization_with_remainder() {
        let original_chunk_with_proof = ChunkWithProof::new(b"ABCDEF123456", 1).unwrap();

        let mut bytes = original_chunk_with_proof
            .to_bytes()
            .expect("should serialize correctly");
        bytes.push(0xFF);

        let (deserialized_chunk_with_proof, remainder) =
            ChunkWithProof::from_bytes(&bytes).expect("should deserialize correctly");

        assert_eq!(original_chunk_with_proof, deserialized_chunk_with_proof);
        assert_eq!(remainder.first().unwrap(), &0xFF);
        assert_eq!(remainder.len(), 1);
    }
}
