//! Core types for a Merkle Trie
mod pointer_block;

use std::fmt::{self, Debug, Display, Formatter};

use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

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
    fn tag(&self) -> u8 {
        match self {
            Trie::Leaf { .. } => 0,
            Trie::Node { .. } => 1,
            Trie::Extension { .. } => 2,
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

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
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
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            0 => {
                let (key, rem) = K::from_bytes(rem)?;
                let (value, rem) = V::from_bytes(rem)?;
                Ok((Trie::Leaf { key, value }, rem))
            }
            1 => {
                let (pointer_block, rem) = PointerBlock::from_bytes(rem)?;
                Ok((
                    Trie::Node {
                        pointer_block: Box::new(pointer_block),
                    },
                    rem,
                ))
            }
            2 => {
                let (affix, rem) = FromBytes::from_bytes(rem)?;
                let (pointer, rem) = Pointer::from_bytes(rem)?;
                Ok((Trie::Extension { affix, pointer }, rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
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
        let root_bytes: Vec<u8> = root.to_bytes()?;
        Ok((Digest::hash(&root_bytes), root))
    }
}
