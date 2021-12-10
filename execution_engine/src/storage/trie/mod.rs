//! Core types for a Merkle Trie
mod pointer_block;

use casper_hashing::{DefaultHasher, Digest};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Display, Formatter};

use casper_types::bytesrepr::{
    self, Bytes, FromBytes, ToBytes, OPTION_NONE_TAG, OPTION_SOME_TAG, U8_SERIALIZED_LENGTH,
};

pub use self::pointer_block::{Pointer, PointerBlock};

pub(crate) use self::pointer_block::{RADIX, USIZE_EXCEEDS_U8};

#[cfg(test)]
pub mod gens;

/// Merkle proofs.
pub mod merkle_proof;
#[cfg(test)]
mod tests;

/// A parent is represented as a pair of a child index and a node or extension.
pub type Parents<K, V> = Vec<(u8, Trie<K, V>)>;

#[derive(FromPrimitive)]
enum TrieTag {
    Leaf = 0,
    Node = 1,
    Extension = 2,
}

const MERKLE_PROOF_TRIE_TAG_LEAF: u8 = 0;
const MERKLE_PROOF_TRIE_TAG_NODE: u8 = 1;
const MERKLE_PROOF_TRIE_TAG_EXTENSION: u8 = 2;
const MERKLE_PROOF_POINTER_TAG_LEAF: u8 = 0;
const MERKLE_PROOF_POINTER_TAG_NODE: u8 = 1;

/// Represents a Merkle Trie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trie<K, V> {
    /// Trie leaf.
    Leaf {
        /// Leaf key.
        key: K,
        /// Leaf value.
        value: V,
    },
    /// Trie node.
    Node {
        /// Node pointer block.
        pointer_block: Box<PointerBlock>,
    },
    /// Trie extension node.
    Extension {
        /// Extension node affix bytes.
        affix: Bytes,
        /// Extension node pointer.
        pointer: Pointer,
    },
}

impl<K, V> Display for Trie<K, V>
where
    K: Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl<K, V> Trie<K, V> {
    fn tag(&self) -> TrieTag {
        match self {
            Trie::Leaf { .. } => TrieTag::Leaf,
            Trie::Node { .. } => TrieTag::Node,
            Trie::Extension { .. } => TrieTag::Extension,
        }
    }

    /// Constructs a [`Trie::Leaf`] from a given key and value.
    pub fn leaf(key: K, value: V) -> Self {
        Trie::Leaf { key, value }
    }

    /// Constructs a [`Trie::Node`] from a given slice of indexed pointers.
    pub fn node(indexed_pointers: &[(u8, Pointer)]) -> Self {
        let pointer_block = PointerBlock::from_indexed_pointers(indexed_pointers);
        let pointer_block = Box::new(pointer_block);
        Trie::Node { pointer_block }
    }

    /// Constructs a [`Trie::Extension`] from a given affix and pointer.
    pub fn extension(affix: Vec<u8>, pointer: Pointer) -> Self {
        Trie::Extension {
            affix: affix.into(),
            pointer,
        }
    }

    /// Gets a reference to the root key of this Trie.
    pub fn key(&self) -> Option<&K> {
        match self {
            Trie::Leaf { key, .. } => Some(key),
            _ => None,
        }
    }
}

impl<K, V> Trie<K, V>
where
    K: ToBytes,
    V: ToBytes,
{
    /// Computes a deterministic hash of a trie object.
    pub fn hash_digest(&self) -> Result<Digest, bytesrepr::Error> {
        let mut hasher = DefaultHasher::new();

        match self {
            Trie::Leaf { key, value } => {
                hasher.update(&[MERKLE_PROOF_TRIE_TAG_LEAF]);
                key.write_bytes(&mut hasher)?;
                value.write_bytes(&mut hasher)?;
            }
            Trie::Node { pointer_block } => {
                hasher.update(&[MERKLE_PROOF_TRIE_TAG_NODE]);

                for maybe_pointer in pointer_block.iter() {
                    match maybe_pointer {
                        Some(Pointer::LeafPointer(leaf)) => {
                            hasher.update(&[OPTION_SOME_TAG]);
                            hasher.update(&[MERKLE_PROOF_POINTER_TAG_LEAF]);
                            hasher.update(&leaf.value());
                        }
                        Some(Pointer::NodePointer(node)) => {
                            hasher.update(&[OPTION_SOME_TAG]);
                            hasher.update(&[MERKLE_PROOF_POINTER_TAG_NODE]);
                            hasher.update(&node.value());
                        }
                        None => hasher.update(&[OPTION_NONE_TAG]),
                    };
                }
            }
            Trie::Extension { affix, pointer } => {
                hasher.update(&[MERKLE_PROOF_TRIE_TAG_EXTENSION]);
                affix.write_bytes(&mut hasher)?;
                pointer.write_bytes(&mut hasher)?;
            }
        }

        Ok(hasher.finalize())
    }
}

impl<K, V> ToBytes for Trie<K, V>
where
    K: ToBytes,
    V: ToBytes,
{
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut ret)?;
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Trie::Leaf { key, value } => key.serialized_length() + value.serialized_length(),
                Trie::Node { pointer_block } => pointer_block.serialized_length(),
                Trie::Extension { affix, pointer } => {
                    affix.serialized_length() + pointer.serialized_length()
                }
            }
    }

    fn write_bytes<W>(&self, writer: &mut W) -> Result<(), bytesrepr::Error>
    where
        W: bytesrepr::Writer,
    {
        writer.write_u8(self.tag() as u8)?;
        match self {
            Trie::Leaf { key, value } => {
                key.write_bytes(writer)?;
                value.write_bytes(writer)?;
            }
            Trie::Node { pointer_block } => pointer_block.write_bytes(writer)?,
            Trie::Extension { affix, pointer } => {
                affix.write_bytes(writer)?;
                pointer.write_bytes(writer)?;
            }
        }
        Ok(())
    }
}

impl<K: FromBytes, V: FromBytes> FromBytes for Trie<K, V> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag_byte, rem) = u8::from_bytes(bytes)?;
        let tag = TrieTag::from_u8(tag_byte).ok_or(bytesrepr::Error::Formatting)?;
        match tag {
            TrieTag::Leaf => {
                let (key, rem) = K::from_bytes(rem)?;
                let (value, rem) = V::from_bytes(rem)?;
                Ok((Trie::Leaf { key, value }, rem))
            }
            TrieTag::Node => {
                let (pointer_block, rem) = PointerBlock::from_bytes(rem)?;
                Ok((
                    Trie::Node {
                        pointer_block: Box::new(pointer_block),
                    },
                    rem,
                ))
            }
            TrieTag::Extension => {
                let (affix, rem) = FromBytes::from_bytes(rem)?;
                let (pointer, rem) = Pointer::from_bytes(rem)?;
                Ok((Trie::Extension { affix, pointer }, rem))
            }
        }
    }
}

pub(crate) mod operations {
    use casper_types::bytesrepr::{self, ToBytes};

    use crate::storage::trie::Trie;
    use casper_hashing::Digest;

    /// Creates a tuple containing an empty root hash and an empty root (a node
    /// with an empty pointer block)
    pub fn create_hashed_empty_trie<K: ToBytes, V: ToBytes>(
    ) -> Result<(Digest, Trie<K, V>), bytesrepr::Error> {
        let root: Trie<K, V> = Trie::Node {
            pointer_block: Default::default(),
        };
        let trie_hash = root.hash_digest()?;
        Ok((trie_hash, root))
    }
}
