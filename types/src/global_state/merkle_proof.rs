use alloc::{collections::VecDeque, vec::Vec};
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};

use super::pointer::Pointer;

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

const TRIE_MERKLE_PROOF_STEP_NODE_ID: u8 = 0;
const TRIE_MERKLE_PROOF_STEP_EXTENSION_ID: u8 = 1;

/// A component of a proof that an entry exists in the Merkle trie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrieMerkleProofStep {
    /// Corresponds to [`Trie::Node`]
    Node {
        /// Hole index.
        hole_index: u8,
        /// Indexed pointers with hole.
        indexed_pointers_with_hole: Vec<(u8, Pointer)>,
    },
    /// Corresponds to [`Trie::Extension`]
    Extension {
        /// Affix bytes.
        affix: Bytes,
    },
}

impl TrieMerkleProofStep {
    /// Constructor for  [`TrieMerkleProofStep::Node`]
    pub fn node(hole_index: u8, indexed_pointers_with_hole: Vec<(u8, Pointer)>) -> Self {
        Self::Node {
            hole_index,
            indexed_pointers_with_hole,
        }
    }

    /// Constructor for  [`TrieMerkleProofStep::Extension`]
    pub fn extension(affix: Vec<u8>) -> Self {
        Self::Extension {
            affix: affix.into(),
        }
    }

    /// Returns a random `TrieMerkleProofStep`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use rand::Rng;

        match rng.gen_range(0..2) {
            0 => {
                let hole_index = rng.gen();
                let indexed_pointers_with_hole = (0..rng.gen_range(0..10))
                    .map(|_| (rng.gen(), Pointer::random(rng)))
                    .collect();
                Self::node(hole_index, indexed_pointers_with_hole)
            }
            1 => {
                let affix = (0..rng.gen_range(0..10)).map(|_| rng.gen()).collect();
                Self::extension(affix)
            }
            _ => unreachable!(),
        }
    }
}

impl ToBytes for TrieMerkleProofStep {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret: Vec<u8> = bytesrepr::allocate_buffer(self)?;
        match self {
            TrieMerkleProofStep::Node {
                hole_index,
                indexed_pointers_with_hole,
            } => {
                ret.push(TRIE_MERKLE_PROOF_STEP_NODE_ID);
                ret.push(*hole_index);
                ret.append(&mut indexed_pointers_with_hole.to_bytes()?)
            }
            TrieMerkleProofStep::Extension { affix } => {
                ret.push(TRIE_MERKLE_PROOF_STEP_EXTENSION_ID);
                ret.append(&mut affix.to_bytes()?)
            }
        };
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        core::mem::size_of::<u8>()
            + match self {
                TrieMerkleProofStep::Node {
                    hole_index,
                    indexed_pointers_with_hole,
                } => {
                    (*hole_index).serialized_length()
                        + (*indexed_pointers_with_hole).serialized_length()
                }
                TrieMerkleProofStep::Extension { affix } => affix.serialized_length(),
            }
    }
}

impl FromBytes for TrieMerkleProofStep {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            TRIE_MERKLE_PROOF_STEP_NODE_ID => {
                let (hole_index, rem): (u8, &[u8]) = FromBytes::from_bytes(rem)?;
                let (indexed_pointers_with_hole, rem): (Vec<(u8, Pointer)>, &[u8]) =
                    FromBytes::from_bytes(rem)?;
                Ok((
                    TrieMerkleProofStep::Node {
                        hole_index,
                        indexed_pointers_with_hole,
                    },
                    rem,
                ))
            }
            TRIE_MERKLE_PROOF_STEP_EXTENSION_ID => {
                let (affix, rem): (_, &[u8]) = FromBytes::from_bytes(rem)?;
                Ok((TrieMerkleProofStep::Extension { affix }, rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// A proof that a node with a specified `key` and `value` is present in the Merkle trie.
/// Given a state hash `x`, one can validate a proof `p` by checking `x == p.compute_state_hash()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrieMerkleProof<K, V> {
    key: K,
    value: V,
    proof_steps: VecDeque<TrieMerkleProofStep>,
}

impl<K, V> TrieMerkleProof<K, V> {
    /// Constructor for [`TrieMerkleProof`]
    pub fn new(key: K, value: V, proof_steps: VecDeque<TrieMerkleProofStep>) -> Self {
        TrieMerkleProof {
            key,
            value,
            proof_steps,
        }
    }

    /// Getter for the key in [`TrieMerkleProof`]
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Getter for the value in [`TrieMerkleProof`]
    pub fn value(&self) -> &V {
        &self.value
    }

    /// Getter for the proof steps in [`TrieMerkleProof`]
    pub fn proof_steps(&self) -> &VecDeque<TrieMerkleProofStep> {
        &self.proof_steps
    }

    /// Transforms a [`TrieMerkleProof`] into the value it contains
    pub fn into_value(self) -> V {
        self.value
    }
}

impl<K, V> ToBytes for TrieMerkleProof<K, V>
where
    K: ToBytes,
    V: ToBytes,
{
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret: Vec<u8> = bytesrepr::allocate_buffer(self)?;
        ret.append(&mut self.key.to_bytes()?);
        ret.append(&mut self.value.to_bytes()?);
        ret.append(&mut self.proof_steps.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.key.serialized_length()
            + self.value.serialized_length()
            + self.proof_steps.serialized_length()
    }
}

impl<K, V> FromBytes for TrieMerkleProof<K, V>
where
    K: FromBytes,
    V: FromBytes,
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, rem): (K, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (value, rem): (V, &[u8]) = FromBytes::from_bytes(rem)?;
        let (proof_steps, rem): (VecDeque<TrieMerkleProofStep>, &[u8]) =
            FromBytes::from_bytes(rem)?;
        Ok((
            TrieMerkleProof {
                key,
                value,
                proof_steps,
            },
            rem,
        ))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn trie_merkle_proof_step_serialization_is_correct(
            step in gens::trie_merkle_proof_step_arb()
        ) {
            bytesrepr::test_serialization_roundtrip(&step)
        }

        #[test]
        fn trie_merkle_proof_serialization_is_correct(
            proof in gens::trie_merkle_proof_arb()
        ) {
            bytesrepr::test_serialization_roundtrip(&proof)
        }
    }
}
