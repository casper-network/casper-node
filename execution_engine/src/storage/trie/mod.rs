//! Core types for a Merkle Trie

use std::{
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    iter::Flatten,
    mem::MaybeUninit,
    slice,
};

use serde::{
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};

use casper_hashing::{ChunkWithProof, Digest};
use casper_types::bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use datasize::DataSize;

#[cfg(test)]
pub mod gens;

/// Merkle proofs.
pub mod merkle_proof;
#[cfg(test)]
mod tests;

pub(crate) const USIZE_EXCEEDS_U8: &str = "usize exceeds u8";
pub(crate) const RADIX: usize = 256;

/// A parent is represented as a pair of a child index and a node or extension.
pub type Parents<K, V> = Vec<(u8, Trie<K, V>)>;

/// Represents a pointer to the next object in a Merkle Trie
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pointer {
    /// Leaf pointer.
    LeafPointer(Digest),
    /// Node pointer.
    NodePointer(Digest),
}

impl Pointer {
    /// Borrows the inner hash from a `Pointer`.
    pub fn hash(&self) -> &Digest {
        match self {
            Pointer::LeafPointer(hash) => hash,
            Pointer::NodePointer(hash) => hash,
        }
    }

    /// Takes ownership of the hash, consuming this `Pointer`.
    pub fn into_hash(self) -> Digest {
        match self {
            Pointer::LeafPointer(hash) => hash,
            Pointer::NodePointer(hash) => hash,
        }
    }

    /// Creates a new owned `Pointer` with a new `Digest`.
    pub fn update(&self, hash: Digest) -> Self {
        match self {
            Pointer::LeafPointer(_) => Pointer::LeafPointer(hash),
            Pointer::NodePointer(_) => Pointer::NodePointer(hash),
        }
    }

    /// Returns the `tag` value for a variant of `Pointer`.
    fn tag(&self) -> u8 {
        match self {
            Pointer::LeafPointer(_) => 0,
            Pointer::NodePointer(_) => 1,
        }
    }
}

impl ToBytes for Pointer {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        self.write_bytes(&mut ret)?;
        Ok(ret)
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH + Digest::LENGTH
    }

    #[inline]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag());
        writer.extend_from_slice(self.hash().as_ref());
        Ok(())
    }
}

impl FromBytes for Pointer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            0 => {
                let (hash, rem) = Digest::from_bytes(rem)?;
                Ok((Pointer::LeafPointer(hash), rem))
            }
            1 => {
                let (hash, rem) = Digest::from_bytes(rem)?;
                Ok((Pointer::NodePointer(hash), rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Type alias for values under pointer blocks.
pub type PointerBlockValue = Option<Pointer>;

/// Type alias for arrays of pointer block values.
pub type PointerBlockArray = [PointerBlockValue; RADIX];

/// Represents the underlying structure of a node in a Merkle Trie
#[derive(Copy, Clone)]
pub struct PointerBlock(PointerBlockArray);

impl Serialize for PointerBlock {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // We are going to use the sparse representation of pointer blocks
        // non-None entries and their indices will be output

        // Create the sequence serializer, reserving the necessary number of slots
        let elements_count = self.0.iter().filter(|element| element.is_some()).count();
        let mut map = serializer.serialize_map(Some(elements_count))?;

        // Store the non-None entries with their indices
        for (index, maybe_pointer_block) in self.0.iter().enumerate() {
            if let Some(pointer_block_value) = maybe_pointer_block {
                map.serialize_entry(&(index as u8), pointer_block_value)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for PointerBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PointerBlockDeserializer;

        impl<'de> Visitor<'de> for PointerBlockDeserializer {
            type Value = PointerBlock;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("sparse representation of a PointerBlock")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut pointer_block = PointerBlock::new();

                // Unpack the sparse representation
                while let Some((index, pointer_block_value)) = access.next_entry::<u8, Pointer>()? {
                    let element = pointer_block.0.get_mut(usize::from(index)).ok_or_else(|| {
                        de::Error::custom(format!("invalid index {} in pointer block value", index))
                    })?;
                    *element = Some(pointer_block_value);
                }

                Ok(pointer_block)
            }
        }
        deserializer.deserialize_map(PointerBlockDeserializer)
    }
}

impl PointerBlock {
    /// No-arg constructor for `PointerBlock`. Delegates to `Default::default()`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Constructs a `PointerBlock` from a slice of indexed `Pointer`s.
    pub fn from_indexed_pointers(indexed_pointers: &[(u8, Pointer)]) -> Self {
        let mut ret = PointerBlock::new();
        for (idx, ptr) in indexed_pointers.iter() {
            ret[*idx as usize] = Some(*ptr);
        }
        ret
    }

    /// Deconstructs a `PointerBlock` into an iterator of indexed `Pointer`s.
    pub fn as_indexed_pointers(&self) -> impl Iterator<Item = (u8, Pointer)> + '_ {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(index, maybe_pointer)| {
                maybe_pointer
                    .map(|value| (index.try_into().expect(USIZE_EXCEEDS_U8), value.to_owned()))
            })
    }

    /// Gets the count of children for this `PointerBlock`.
    pub fn child_count(&self) -> usize {
        self.as_indexed_pointers().count()
    }
}

impl From<PointerBlockArray> for PointerBlock {
    fn from(src: PointerBlockArray) -> Self {
        PointerBlock(src)
    }
}

impl PartialEq for PointerBlock {
    #[inline]
    fn eq(&self, other: &PointerBlock) -> bool {
        self.0[..] == other.0[..]
    }
}

impl Eq for PointerBlock {}

impl Default for PointerBlock {
    fn default() -> Self {
        PointerBlock([Default::default(); RADIX])
    }
}

impl ToBytes for PointerBlock {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        for pointer in self.0.iter() {
            result.append(&mut pointer.to_bytes()?);
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.iter().map(ToBytes::serialized_length).sum()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        for pointer in self.0.iter() {
            pointer.write_bytes(writer)?;
        }
        Ok(())
    }
}

impl FromBytes for PointerBlock {
    fn from_bytes(mut bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let pointer_block_array = {
            // With MaybeUninit here we can avoid default initialization of result array below.
            let mut result: MaybeUninit<PointerBlockArray> = MaybeUninit::uninit();
            let result_ptr = result.as_mut_ptr() as *mut PointerBlockValue;
            for i in 0..RADIX {
                let (t, remainder) = match FromBytes::from_bytes(bytes) {
                    Ok(success) => success,
                    Err(error) => {
                        for j in 0..i {
                            unsafe { result_ptr.add(j).drop_in_place() }
                        }
                        return Err(error);
                    }
                };
                unsafe { result_ptr.add(i).write(t) };
                bytes = remainder;
            }
            unsafe { result.assume_init() }
        };
        Ok((PointerBlock(pointer_block_array), bytes))
    }
}

impl core::ops::Index<usize> for PointerBlock {
    type Output = PointerBlockValue;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        let PointerBlock(dat) = self;
        &dat[index]
    }
}

impl core::ops::IndexMut<usize> for PointerBlock {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let PointerBlock(dat) = self;
        &mut dat[index]
    }
}

impl core::ops::Index<core::ops::Range<usize>> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::Range<usize>) -> &[PointerBlockValue] {
        let &PointerBlock(ref dat) = self;
        &dat[index]
    }
}

impl core::ops::Index<core::ops::RangeTo<usize>> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::RangeTo<usize>) -> &[PointerBlockValue] {
        let &PointerBlock(ref dat) = self;
        &dat[index]
    }
}

impl core::ops::Index<core::ops::RangeFrom<usize>> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::RangeFrom<usize>) -> &[PointerBlockValue] {
        let &PointerBlock(ref dat) = self;
        &dat[index]
    }
}

impl core::ops::Index<core::ops::RangeFull> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::RangeFull) -> &[PointerBlockValue] {
        let &PointerBlock(ref dat) = self;
        &dat[index]
    }
}

impl ::std::fmt::Debug for PointerBlock {
    #[allow(clippy::assertions_on_constants)]
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        assert!(RADIX > 1, "RADIX must be > 1");
        write!(f, "{}([", stringify!(PointerBlock))?;
        write!(f, "{:?}", self.0[0])?;
        for item in self.0[1..].iter() {
            write!(f, ", {:?}", item)?;
        }
        write!(f, "])")
    }
}

/// Represents a Merkle Tree or a chunk of data with attached proof.
/// Chunk with attached proof is used when the requested
/// trie is larger than [ChunkWithProof::CHUNK_SIZE_BYTES].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrieOrChunk {
    /// Represents a Merkle Trie.
    Trie(Bytes),
    /// Represents a chunk of data with attached proof.
    ChunkWithProof(ChunkWithProof),
}

impl Display for TrieOrChunk {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            TrieOrChunk::Trie(trie) => f.debug_tuple("Trie").field(trie).finish(),
            TrieOrChunk::ChunkWithProof(chunk) => f
                .debug_struct("ChunkWithProof")
                .field("index", &chunk.proof().index())
                .field("hash", &chunk.proof().root_hash())
                .finish(),
        }
    }
}

impl TrieOrChunk {
    const TRIE_TAG: u8 = 0;
    const CHUNK_TAG: u8 = 1;

    fn tag(&self) -> u8 {
        match self {
            TrieOrChunk::Trie(_) => Self::TRIE_TAG,
            TrieOrChunk::ChunkWithProof(_) => Self::CHUNK_TAG,
        }
    }
}

impl ToBytes for TrieOrChunk {
    fn write_bytes(&self, buf: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        buf.push(self.tag());

        match self {
            TrieOrChunk::Trie(trie) => {
                buf.append(&mut trie.to_bytes()?);
            }
            TrieOrChunk::ChunkWithProof(chunk) => {
                buf.append(&mut chunk.to_bytes()?);
            }
        }

        Ok(())
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut ret)?;
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TrieOrChunk::Trie(trie) => trie.serialized_length(),
                TrieOrChunk::ChunkWithProof(chunk) => chunk.serialized_length(),
            }
    }
}

impl FromBytes for TrieOrChunk {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            Self::TRIE_TAG => {
                let (trie_bytes, rem) = Bytes::from_bytes(rem)?;
                Ok((TrieOrChunk::Trie(trie_bytes), rem))
            }
            Self::CHUNK_TAG => {
                let (chunk, rem) = ChunkWithProof::from_bytes(rem)?;
                Ok((TrieOrChunk::ChunkWithProof(chunk), rem))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Represents the ID of a `TrieOrChunk` - containing the index and the root hash.
/// The root hash is the hash of the trie node as a whole.
/// The index is the index of a chunk if the node's size is too large and requires chunking. For
/// small nodes, it's always 0.
#[derive(DataSize, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TrieOrChunkId(pub u64, pub Digest);

impl TrieOrChunkId {
    /// Returns the trie key part of the ID.
    pub fn digest(&self) -> &Digest {
        &self.1
    }

    /// Given a serialized ID, deserializes it for display purposes.
    pub fn fmt_serialized(f: &mut Formatter, serialized_id: &[u8]) -> fmt::Result {
        match bincode::deserialize::<Self>(serialized_id) {
            Ok(ref trie_or_chunk_id) => Display::fmt(trie_or_chunk_id, f),
            Err(_) => f.write_str("<invalid>"),
        }
    }
}

/// Helper struct to on-demand deserialize a trie or chunk ID for display purposes.
pub struct TrieOrChunkIdDisplay<'a>(pub &'a [u8]);

impl<'a> Display for TrieOrChunkIdDisplay<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        TrieOrChunkId::fmt_serialized(f, self.0)
    }
}

impl Display for TrieOrChunkId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.0, self.1)
    }
}

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
    pub(crate) const LEAF_TAG: u8 = 0;
    const NODE_TAG: u8 = 1;
    const EXTENSION_TAG: u8 = 2;

    fn tag(&self) -> u8 {
        match self {
            Trie::Leaf { .. } => Self::LEAF_TAG,
            Trie::Node { .. } => Self::NODE_TAG,
            Trie::Extension { .. } => Self::EXTENSION_TAG,
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

    /// Returns the hash of this Trie.
    pub fn trie_hash(&self) -> Result<Digest, bytesrepr::Error>
    where
        Self: ToBytes,
    {
        self.to_bytes()
            .map(|bytes| hash_bytes_into_chunks_if_necessary(&bytes))
    }

    /// Returns a pointer block, if possible.
    pub fn as_pointer_block(&self) -> Option<&PointerBlock> {
        if let Self::Node { pointer_block } = self {
            Some(pointer_block.as_ref())
        } else {
            None
        }
    }

    /// Returns an iterator over descendants of the trie.
    pub fn iter_descendants(&self) -> DescendantsIterator {
        match self {
            Trie::<K, V>::Leaf { .. } => DescendantsIterator::ZeroOrOne(None),
            Trie::Node { pointer_block } => DescendantsIterator::PointerBlock {
                iter: pointer_block.0.iter().flatten(),
            },
            Trie::Extension { pointer, .. } => {
                DescendantsIterator::ZeroOrOne(Some(pointer.into_hash()))
            }
        }
    }
}

/// An iterator over the descendants of a trie node.
pub enum DescendantsIterator<'a> {
    /// A leaf (zero descendants) or extension (one descendant) being iterated.
    ZeroOrOne(Option<Digest>),
    /// A pointer block being iterated.
    PointerBlock {
        /// An iterator over the non-None entries of the `PointerBlock`.
        iter: Flatten<slice::Iter<'a, Option<Pointer>>>,
    },
}

impl<'a> Iterator for DescendantsIterator<'a> {
    type Item = Digest;

    fn next(&mut self) -> Option<Self::Item> {
        match *self {
            DescendantsIterator::ZeroOrOne(ref mut maybe_digest) => maybe_digest.take(),
            DescendantsIterator::PointerBlock { ref mut iter } => {
                iter.next().map(|pointer| *pointer.hash())
            }
        }
    }
}

/// Hash bytes into chunks if necessary.
pub(crate) fn hash_bytes_into_chunks_if_necessary(bytes: &[u8]) -> Digest {
    if bytes.len() <= ChunkWithProof::CHUNK_SIZE_BYTES {
        Digest::hash(bytes)
    } else {
        Digest::hash_merkle_tree(
            bytes
                .chunks(ChunkWithProof::CHUNK_SIZE_BYTES)
                .map(Digest::hash),
        )
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
            Self::LEAF_TAG => {
                let (key, rem) = K::from_bytes(rem)?;
                let (value, rem) = V::from_bytes(rem)?;
                Ok((Trie::Leaf { key, value }, rem))
            }
            Self::NODE_TAG => {
                let (pointer_block, rem) = PointerBlock::from_bytes(rem)?;
                Ok((
                    Trie::Node {
                        pointer_block: Box::new(pointer_block),
                    },
                    rem,
                ))
            }
            Self::EXTENSION_TAG => {
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
