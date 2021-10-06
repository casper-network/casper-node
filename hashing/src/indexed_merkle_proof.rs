use crate::Digest;
use casper_types::{
    allocate_buffer,
    bytesrepr::{FromBytes, ToBytes},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use itertools::Itertools;

use crate::error;

#[derive(PartialEq, Debug, JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedMerkleProof {
    index: u64,
    count: u64,
    merkle_proof: Vec<Digest>,
}

impl ToBytes for IndexedMerkleProof {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let mut result = allocate_buffer(self)?;
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
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let ((index, count, merkle_proof), remainder) =
            <(u64, u64, Vec<Digest>)>::from_bytes(bytes)?;

        Ok((
            IndexedMerkleProof {
                index,
                count,
                merkle_proof,
            },
            remainder,
        ))
    }
}

impl IndexedMerkleProof {
    pub(crate) fn new<I>(
        leaves: I,
        index: u64,
    ) -> Result<IndexedMerkleProof, error::MerkleConstructionError>
    where
        I: IntoIterator<Item = Digest>,
        I::IntoIter: ExactSizeIterator,
    {
        enum HashOrProof {
            Hash(Digest),
            Proof(Vec<Digest>),
        }
        use std::convert::TryInto;

        use HashOrProof::{Hash, Proof};

        let leaves = leaves.into_iter();
        let count: u64 = leaves
            .len()
            .try_into()
            .expect("Unable to process more than u64::MAX leaves");

        let maybe_proof = leaves
            .enumerate()
            .map(|(i, hash)| {
                if i as u64 == index {
                    Proof(vec![hash])
                } else {
                    Hash(hash)
                }
            })
            .tree_fold1(|x, y| match (x, y) {
                (Hash(hash_x), Hash(hash_y)) => Hash(Digest::hash_pair(&hash_x, &hash_y)),
                (Hash(hash), Proof(mut proof)) | (Proof(mut proof), Hash(hash)) => {
                    proof.push(hash);
                    Proof(proof)
                }
                (Proof(_), Proof(_)) => unreachable!(),
            });
        match maybe_proof {
            None => {
                if index != 0 {
                    Err(error::MerkleConstructionError::EmptyProofMustHaveIndex { index })
                } else {
                    Ok(IndexedMerkleProof {
                        index: 0,
                        count: 0,
                        merkle_proof: Vec::new(),
                    })
                }
            }
            Some(Hash(_)) => Err(error::MerkleConstructionError::IndexOutOfBounds { count, index }),
            Some(Proof(merkle_proof)) => Ok(IndexedMerkleProof {
                index,
                count,
                merkle_proof,
            }),
        }
    }

    #[allow(unused)]
    pub(crate) fn root_hash(&self) -> Digest {
        use blake2::{
            digest::{Update, VariableOutput},
            VarBlake2b,
        };

        let IndexedMerkleProof {
            index: _,
            count,
            merkle_proof,
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
            let mut acc = leaf_hash;

            for hash in hashes {
                let mut hasher = VarBlake2b::new(Digest::LENGTH).unwrap();
                if (path & 1) == 1 {
                    hasher.update(hash);
                    hasher.update(&acc);
                } else {
                    hasher.update(&acc);
                    hasher.update(hash);
                }
                hasher.finalize_variable(|slice| {
                    acc.0.copy_from_slice(slice);
                });
                path >>= 1;
            }
            acc
        } else {
            Digest::SENTINEL_MERKLE_TREE
        };

        // The Merkle root is the hash of the count with the raw root.
        Digest::hash_pair(count.to_le_bytes(), raw_root)
    }

    pub(crate) fn merkle_proof(&self) -> &[Digest] {
        &self.merkle_proof
    }

    // Proof lengths are never bigger than 65, so we can use a u8 here
    // The reason they are never bigger than 65 is because we are using 64 bit counts
    pub(crate) fn compute_expected_proof_length(&self) -> u64 {
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

    #[allow(unused)]
    fn verify(&self) -> Result<(), error::MerkleVerificationError> {
        if !((self.count == 0 && self.index == 0) || self.index < self.count) {
            return Err(error::MerkleVerificationError::IndexOutOfBounds {
                count: self.count,
                index: self.index,
            });
        }
        let expected_proof_length = self.compute_expected_proof_length();
        if self.merkle_proof.len() != expected_proof_length as usize {
            return Err(error::MerkleVerificationError::UnexpectedProofLength {
                count: self.count,
                index: self.index,
                expected_proof_length,
                actual_proof_length: self.merkle_proof.len(),
            });
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_merkle_proof(&mut self, merkle_proof: Vec<Digest>) {
        self.merkle_proof = merkle_proof;
    }
}

#[cfg(test)]
mod test {
    use casper_types::bytesrepr::{FromBytes, ToBytes};
    use proptest::prelude::{prop_assert, prop_assert_eq};
    use proptest_attr_macro::proptest;
    use rand::Rng;

    use crate::{error, indexed_merkle_proof::IndexedMerkleProof, Digest};

    fn dummy_indexed_merkle_proof() -> IndexedMerkleProof {
        let mut rng = rand::thread_rng();
        let leaf_count: u64 = rng.gen_range(1..100);
        let index = rng.gen_range(0..leaf_count);
        let leaves: Vec<Digest> = (0..leaf_count)
            .map(|i| Digest::hash(i.to_le_bytes()))
            .collect();
        IndexedMerkleProof::new(leaves.iter().cloned(), index)
            .expect("should create indexed merkle proof")
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
                indexed_merkle_proof.merkle_proof().len() as u64
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
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(error::MerkleVerificationError::IndexOutOfBounds {
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
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(error::MerkleVerificationError::UnexpectedProofLength {
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
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(error::MerkleVerificationError::UnexpectedProofLength {
                count: 0,
                index: 0,
                expected_proof_length: 0,
                actual_proof_length: 3
            })
        )
    }

    #[test]
    fn empty_out_of_bounds_index() {
        let out_of_bounds_indexed_merkle_proof = IndexedMerkleProof {
            index: 23,
            count: 0,
            merkle_proof: vec![],
        };
        assert_eq!(
            out_of_bounds_indexed_merkle_proof.verify(),
            Err(error::MerkleVerificationError::IndexOutOfBounds {
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
        };
        let _hash = indexed_merkle_proof.root_hash();
    }

    #[test]
    fn empty_proof() {
        let empty_merkle_root = Digest::hash_merkle_tree(vec![]);
        assert_eq!(
            empty_merkle_root,
            Digest::hash_pair(0u64.to_le_bytes(), Digest::SENTINEL_MERKLE_TREE)
        );
        let indexed_merkle_proof = IndexedMerkleProof {
            index: 0,
            count: 0,
            merkle_proof: vec![],
        };
        assert_eq!(indexed_merkle_proof.verify(), Ok(()));
        assert_eq!(indexed_merkle_proof.root_hash(), empty_merkle_root);
    }

    #[proptest]
    fn expected_proof_length_le_65(index: u64, count: u64) {
        let indexed_merkle_proof = IndexedMerkleProof {
            index,
            count,
            merkle_proof: vec![],
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
                Digest::hash_pair(&left, &proof[last])
            } else {
                let right =
                    compute_raw_root_from_proof(index - half, leaf_count - half, &proof[..last]);
                Digest::hash_pair(&proof[last], &right)
            }
        }

        let raw_root = compute_raw_root_from_proof(index, count, proof);
        Digest::hash_pair(count.to_le_bytes(), raw_root)
    }

    /// Construct an `IndexedMerkleProof` with a proof of zero digests.
    fn test_indexed_merkle_proof(index: u64, count: u64) -> IndexedMerkleProof {
        let mut indexed_merkle_proof = IndexedMerkleProof {
            index,
            count,
            merkle_proof: vec![],
        };
        let expected_proof_length = indexed_merkle_proof.compute_expected_proof_length();
        indexed_merkle_proof.merkle_proof = std::iter::repeat_with(|| Digest([0u8; 32]))
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

        // Check that proof with index greater than count fails to deserialize
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.index += 1;
        let json = serde_json::to_string(&indexed_merkle_proof).unwrap();
        serde_json::from_str::<IndexedMerkleProof>(&json).expect("should deserialize correctly");

        // Check that proof with incorrect length fails to deserialize
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

        // Check that proof with index greater than count fails to deserialize
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.index += 1;
        let bytes = indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");
        IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");

        // Check that proof with incorrect length fails to deserialize
        let mut indexed_merkle_proof = test_indexed_merkle_proof(10, 10);
        indexed_merkle_proof.merkle_proof.push(Digest::hash("XXX"));
        let bytes = indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");
        IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");
    }

    #[test]
    fn bytesrepr_serialization() {
        let original_indexed_merkle_proof = dummy_indexed_merkle_proof();

        let bytes = original_indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");

        let (deserialized_indexed_merkle_proof, remainder) =
            IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");

        assert_eq!(
            original_indexed_merkle_proof,
            deserialized_indexed_merkle_proof
        );
        assert!(remainder.is_empty());
    }

    #[test]
    fn bytesrepr_serialization_with_remainder() {
        let original_indexed_merkle_proof = dummy_indexed_merkle_proof();

        let mut bytes = original_indexed_merkle_proof
            .to_bytes()
            .expect("should serialize correctly");
        bytes.push(0xFF);

        let (deserialized_indexed_merkle_proof, remainder) =
            IndexedMerkleProof::from_bytes(&bytes).expect("should deserialize correctly");

        assert_eq!(
            original_indexed_merkle_proof,
            deserialized_indexed_merkle_proof
        );
        assert_eq!(remainder.first().unwrap(), &0xFF);
        assert_eq!(remainder.len(), 1);
    }
}
