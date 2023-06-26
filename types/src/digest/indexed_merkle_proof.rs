//! Constructing and validating indexed Merkle proofs.
use alloc::{string::ToString, vec::Vec};
use core::convert::TryInto;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use itertools::Itertools;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Digest, MerkleConstructionError, MerkleVerificationError};
use crate::bytesrepr::{self, FromBytes, ToBytes};

/// A Merkle proof of the given chunk.
#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IndexedMerkleProof {
    index: u64,
    count: u64,
    merkle_proof: Vec<Digest>,
    #[cfg_attr(any(feature = "once_cell", test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    root_hash: OnceCell<Digest>,
}

impl ToBytes for IndexedMerkleProof {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.index.to_bytes()?);
        result.append(&mut self.count.to_bytes()?);
        result.append(&mut self.merkle_proof.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.index.serialized_length()
            + self.count.serialized_length()
            + self.merkle_proof.serialized_length()
    }
}

impl FromBytes for IndexedMerkleProof {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (index, remainder) = FromBytes::from_bytes(bytes)?;
        let (count, remainder) = FromBytes::from_bytes(remainder)?;
        let (merkle_proof, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            IndexedMerkleProof {
                index,
                count,
                merkle_proof,
                #[cfg(any(feature = "once_cell", test))]
                root_hash: OnceCell::new(),
            },
            remainder,
        ))
    }
}

impl IndexedMerkleProof {
    /// Attempts to construct a new instance.
    pub fn new<I>(leaves: I, index: u64) -> Result<IndexedMerkleProof, MerkleConstructionError>
    where
        I: IntoIterator<Item = Digest>,
        I::IntoIter: ExactSizeIterator,
    {
        use HashOrProof::{Hash as H, Proof as P};

        enum HashOrProof {
            Hash(Digest),
            Proof(Vec<Digest>),
        }

        let leaves = leaves.into_iter();
        let count: u64 =
            leaves
                .len()
                .try_into()
                .map_err(|_| MerkleConstructionError::TooManyLeaves {
                    count: leaves.len().to_string(),
                })?;

        let maybe_proof = leaves
            .enumerate()
            .map(|(i, hash)| {
                if i as u64 == index {
                    P(vec![hash])
                } else {
                    H(hash)
                }
            })
            .tree_fold1(|x, y| match (x, y) {
                (H(hash_x), H(hash_y)) => H(Digest::hash_pair(hash_x, hash_y)),
                (H(hash), P(mut proof)) | (P(mut proof), H(hash)) => {
                    proof.push(hash);
                    P(proof)
                }
                (P(_), P(_)) => unreachable!(),
            });

        match maybe_proof {
            None | Some(H(_)) => Err(MerkleConstructionError::IndexOutOfBounds { count, index }),
            Some(P(merkle_proof)) => Ok(IndexedMerkleProof {
                index,
                count,
                merkle_proof,
                #[cfg(any(feature = "once_cell", test))]
                root_hash: OnceCell::new(),
            }),
        }
    }

    /// Returns the index.
    pub fn index(&self) -> u64 {
        self.index
    }

    /// Returns the total count of chunks.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Returns the root hash of this proof (i.e. the index hashed with the Merkle root hash).
    ///
    /// Note that with the `once_cell` feature enabled (generally done by enabling the `std`
    /// feature), the root hash is memoized, and hence calling this method is cheap after the first
    /// call.  Without `once_cell` enabled, every call to this method calculates the root hash.
    pub fn root_hash(&self) -> Digest {
        #[cfg(any(feature = "once_cell", test))]
        return *self.root_hash.get_or_init(|| self.compute_root_hash());

        #[cfg(not(any(feature = "once_cell", test)))]
        self.compute_root_hash()
    }

    /// Returns the full collection of hash digests of the proof.
    pub fn merkle_proof(&self) -> &[Digest] {
        &self.merkle_proof
    }

    /// Attempts to verify self.
    pub fn verify(&self) -> Result<(), MerkleVerificationError> {
        if self.index >= self.count {
            return Err(MerkleVerificationError::IndexOutOfBounds {
                count: self.count,
                index: self.index,
            });
        }
        let expected_proof_length = self.compute_expected_proof_length();
        if self.merkle_proof.len() != expected_proof_length as usize {
            return Err(MerkleVerificationError::UnexpectedProofLength {
                count: self.count,
                index: self.index,
                expected_proof_length,
                actual_proof_length: self.merkle_proof.len(),
            });
        }
        Ok(())
    }

    fn compute_root_hash(&self) -> Digest {
        let IndexedMerkleProof {
            count,
            merkle_proof,
            ..
        } = self;

        let mut hashes = merkle_proof.iter();
        let raw_root = if let Some(leaf_hash) = hashes.next().cloned() {
            // Compute whether to hash left or right for the elements of the Merkle proof.
            // This gives a path to the value with the specified index.
            // We represent this path as a sequence of 64 bits. 1 here means "hash right".
            let mut path: u64 = 0;
            let mut n = self.count;
            let mut i = self.index;
            while n > 1 {
                path <<= 1;
                let pivot = 1u64 << (63 - (n - 1).leading_zeros());
                if i < pivot {
                    n = pivot;
                } else {
                    path |= 1;
                    n -= pivot;
                    i -= pivot;
                }
            }

            // Compute the raw Merkle root by hashing the proof from leaf hash up.
            hashes.fold(leaf_hash, |acc, hash| {
                let digest = if (path & 1) == 1 {
                    Digest::hash_pair(hash, acc)
                } else {
                    Digest::hash_pair(acc, hash)
                };
                path >>= 1;
                digest
            })
        } else {
            Digest::SENTINEL_MERKLE_TREE
        };

        // The Merkle root is the hash of the count with the raw root.
        Digest::hash_merkle_root(*count, raw_root)
    }

    // Proof lengths are never bigger than 65 is because we are using 64 bit counts
    fn compute_expected_proof_length(&self) -> u8 {
        if self.count == 0 {
            return 0;
        }
        let mut l = 1;
        let mut n = self.count;
        let mut i = self.index;
        while n > 1 {
            let pivot = 1u64 << (63 - (n - 1).leading_zeros());
            if i < pivot {
                n = pivot;
            } else {
                n -= pivot;
                i -= pivot;
            }
            l += 1;
        }
        l
    }

    #[cfg(test)]
    pub fn inject_merkle_proof(&mut self, merkle_proof: Vec<Digest>) {
        self.merkle_proof = merkle_proof;
    }
}

#[cfg(test)]
mod tests {
    use once_cell::sync::OnceCell;
    use proptest::prelude::{prop_assert, prop_assert_eq};
    use proptest_attr_macro::proptest;
    use rand::{distributions::Standard, Rng};

    use crate::{
        bytesrepr::{self, FromBytes, ToBytes},
        Digest, IndexedMerkleProof, MerkleVerificationError,
    };

    fn random_indexed_merkle_proof() -> IndexedMerkleProof {
        let mut rng = rand::thread_rng();
        let leaf_count: u64 = rng.gen_range(1..100);
        let index = rng.gen_range(0..leaf_count);
        let leaves: Vec<Digest> = (0..leaf_count)
            .map(|i| Digest::hash(i.to_le_bytes()))
            .collect();
        IndexedMerkleProof::new(leaves.iter().cloned(), index)
            .expect("should create indexed Merkle proof")
    }

    #[test]
    fn test_merkle_proofs() {
        let mut rng = rand::thread_rng();
        for _ in 0..20 {
            let leaf_count: u64 = rng.gen_range(1..100);
            let index = rng.gen_range(0..leaf_count);
            let leaves: Vec<Digest> = (0..leaf_count)
                .map(|i| Digest::hash(i.to_le_bytes()))
                .collect();
            let root = Digest::hash_merkle_tree(leaves.clone());
            let indexed_merkle_proof = IndexedMerkleProof::new(leaves.clone(), index).unwrap();
            assert_eq!(
                indexed_merkle_proof.compute_expected_proof_length(),
                indexed_merkle_proof.merkle_proof().len() as u8
            );
            assert_eq!(indexed_merkle_proof.verify(), Ok(()));
            assert_eq!(leaf_count, indexed_merkle_proof.count);
            assert_eq!(leaves[index as usize], indexed_merkle_proof.merkle_proof[0]);
            assert_eq!(root, indexed_merkle_proof.root_hash());
        }
    }

    #[test]
    fn out_of_bounds_index() {
        let out_of_bounds_indexed_merkle_proof = IndexedMerkleProof {
            index: 23,
            count: 4,
            merkle_proof: vec![Digest([0u8; 32]); 3],
            root_hash: OnceCell::new(),
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(MerkleVerificationError::IndexOutOfBounds {
                count: 4,
                index: 23
            })
        )
    }

    #[test]
    fn unexpected_proof_length() {
        let out_of_bounds_indexed_merkle_proof = IndexedMerkleProof {
            index: 1235,
            count: 5647,
            merkle_proof: vec![Digest([0u8; 32]); 13],
            root_hash: OnceCell::new(),
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(MerkleVerificationError::UnexpectedProofLength {
                count: 5647,
                index: 1235,
                expected_proof_length: 14,
                actual_proof_length: 13
            })
        )
    }

    #[test]
    fn empty_unexpected_proof_length() {
        let out_of_bounds_indexed_merkle_proof = IndexedMerkleProof {
            index: 0,
            count: 0,
            merkle_proof: vec![Digest([0u8; 32]); 3],
            root_hash: OnceCell::new(),
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(MerkleVerificationError::IndexOutOfBounds { count: 0, index: 0 })
        )
    }

    #[test]
    fn empty_out_of_bounds_index() {
        let out_of_bounds_indexed_merkle_proof = IndexedMerkleProof {
            index: 23,
            count: 0,
            merkle_proof: vec![],
            root_hash: OnceCell::new(),
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(MerkleVerificationError::IndexOutOfBounds {
                count: 0,
                index: 23
            })
        )
    }

    #[test]
    fn deep_proof_doesnt_kill_stack() {
        const PROOF_LENGTH: usize = 63;
        let indexed_merkle_proof = IndexedMerkleProof {
            index: 42,
            count: 1 << (PROOF_LENGTH - 1),
            merkle_proof: vec![Digest([0u8; Digest::LENGTH]); PROOF_LENGTH],
            root_hash: OnceCell::new(),
        };
        let _hash = indexed_merkle_proof.root_hash();
    }

    #[test]
    fn empty_proof() {
        let empty_merkle_root = Digest::hash_merkle_tree(vec![]);
        assert_eq!(empty_merkle_root, Digest::SENTINEL_MERKLE_TREE);
        let indexed_merkle_proof = IndexedMerkleProof {
            index: 0,
            count: 0,
            merkle_proof: vec![],
            root_hash: OnceCell::new(),
        };
        assert!(indexed_merkle_proof.verify().is_err());
    }

    #[proptest]
    fn expected_proof_length_le_65(index: u64, count: u64) {
        let indexed_merkle_proof = IndexedMerkleProof {
            index,
            count,
            merkle_proof: vec![],
            root_hash: OnceCell::new(),
        };
        prop_assert!(indexed_merkle_proof.compute_expected_proof_length() <= 65);
    }

    fn reference_root_from_proof(index: u64, count: u64, proof: &[Digest]) -> Digest {
        fn compute_raw_root_from_proof(index: u64, leaf_count: u64, proof: &[Digest]) -> Digest {
            if leaf_count == 0 {
                return Digest::SENTINEL_MERKLE_TREE;
            }
            if leaf_count == 1 {
                return proof[0];
            }
            let half = 1u64 << (63 - (leaf_count - 1).leading_zeros());
            let last = proof.len() - 1;
            if index < half {
                let left = compute_raw_root_from_proof(index, half, &proof[..last]);
                Digest::hash_pair(left, proof[last])
            } else {
                let right =
                    compute_raw_root_from_proof(index - half, leaf_count - half, &proof[..last]);
                Digest::hash_pair(proof[last], right)
            }
        }

        let raw_root = compute_raw_root_from_proof(index, count, proof);
        Digest::hash_merkle_root(count, raw_root)
    }

    /// Construct an `IndexedMerkleProof` with a proof of zero digests.
    fn test_indexed_merkle_proof(index: u64, count: u64) -> IndexedMerkleProof {
        let mut indexed_merkle_proof = IndexedMerkleProof {
            index,
            count,
            merkle_proof: vec![],
            root_hash: OnceCell::new(),
        };
        let expected_proof_length = indexed_merkle_proof.compute_expected_proof_length();
        indexed_merkle_proof.merkle_proof = rand::thread_rng()
            .sample_iter(Standard)
            .take(expected_proof_length as usize)
            .collect();
        indexed_merkle_proof
    }

    #[proptest]
    fn root_from_proof_agrees_with_recursion(index: u64, count: u64) {
        let indexed_merkle_proof = test_indexed_merkle_proof(index, count);
        prop_assert_eq!(
            indexed_merkle_proof.root_hash(),
            reference_root_from_proof(
                indexed_merkle_proof.index,
                indexed_merkle_proof.count,
                indexed_merkle_proof.merkle_proof(),
            ),
            "Result did not agree with reference implementation.",
        );
    }

    #[test]
    fn root_from_proof_agrees_with_recursion_2147483648_4294967297() {
        let indexed_merkle_proof = test_indexed_merkle_proof(2147483648, 4294967297);
        assert_eq!(
            indexed_merkle_proof.root_hash(),
            reference_root_from_proof(
                indexed_merkle_proof.index,
                indexed_merkle_proof.count,
                indexed_merkle_proof.merkle_proof(),
            ),
            "Result did not agree with reference implementation.",
        );
    }

    #[test]
    fn serde_deserialization_of_malformed_proof_should_work() {
        let indexed_merkle_proof = test_indexed_merkle_proof(10, 10);

        let json = serde_json::to_string(&indexed_merkle_proof).unwrap();
        assert_eq!(
            indexed_merkle_proof,
            serde_json::from_str::<IndexedMerkleProof>(&json)
                .expect("should deserialize correctly")
        );

        // Check that proof with index greater than count deserializes correctly
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.index += 1;
        let json = serde_json::to_string(&indexed_merkle_proof).unwrap();
        serde_json::from_str::<IndexedMerkleProof>(&json).expect("should deserialize correctly");

        // Check that proof with incorrect length deserializes correctly
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.merkle_proof.push(Digest::hash("XXX"));
        let json = serde_json::to_string(&indexed_merkle_proof).unwrap();
        serde_json::from_str::<IndexedMerkleProof>(&json).expect("should deserialize correctly");
    }

    #[test]
    fn bytesrepr_deserialization_of_malformed_proof_should_work() {
        let indexed_merkle_proof = test_indexed_merkle_proof(10, 10);

        let bytes = indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");
        IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");

        // Check that proof with index greater than count deserializes correctly
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.index += 1;
        let bytes = indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");
        IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");

        // Check that proof with incorrect length deserializes correctly
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.merkle_proof.push(Digest::hash("XXX"));
        let bytes = indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");
        IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");
    }

    #[test]
    fn bytesrepr_serialization() {
        let indexed_merkle_proof = random_indexed_merkle_proof();
        bytesrepr::test_serialization_roundtrip(&indexed_merkle_proof);
    }
}
