//! Core types for a Merkle Trie

use std::{
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    iter::Flatten,
    mem::MaybeUninit,
    slice,
};

use datasize::DataSize;
use either::Either;
use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};
use serde::{
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};

use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    global_state::Pointer,
    Digest,
};

#[cfg(test)]
pub mod gens;

#[cfg(test)]
mod tests;

pub(crate) const USIZE_EXCEEDS_U8: &str = "usize exceeds u8";
pub(crate) const RADIX: usize = 256;

/// A parent is represented as a pair of a child index and a node or extension.
pub type Parents<K, V> = Vec<(u8, Trie<K, V>)>;

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
        let PointerBlock(dat) = self;
        &dat[index]
    }
}

impl core::ops::Index<core::ops::RangeTo<usize>> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::RangeTo<usize>) -> &[PointerBlockValue] {
        let PointerBlock(dat) = self;
        &dat[index]
    }
}

impl core::ops::Index<core::ops::RangeFrom<usize>> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::RangeFrom<usize>) -> &[PointerBlockValue] {
        let PointerBlock(dat) = self;
        &dat[index]
    }
}

impl core::ops::Index<core::ops::RangeFull> for PointerBlock {
    type Output = [PointerBlockValue];

    #[inline]
    fn index(&self, index: core::ops::RangeFull) -> &[PointerBlockValue] {
        let PointerBlock(dat) = self;
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

/// Newtype representing a trie node in its raw form without deserializing into `Trie`.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, DataSize)]
pub struct TrieRaw(Bytes);

impl TrieRaw {
    /// Constructs an instance of [`TrieRaw`].
    pub fn new(bytes: Bytes) -> Self {
        TrieRaw(bytes)
    }

    /// Consumes self and returns inner bytes.
    pub fn into_inner(self) -> Bytes {
        self.0
    }

    /// Returns a reference inner bytes.
    pub fn inner(&self) -> &Bytes {
        &self.0
    }

    /// Returns a hash of the inner bytes.
    pub fn hash(&self) -> Digest {
        Digest::hash_into_chunks_if_necessary(self.inner())
    }
}

impl ToBytes for TrieRaw {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for TrieRaw {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, rem) = Bytes::from_bytes(bytes)?;
        Ok((TrieRaw(bytes), rem))
    }
}

/// Represents all possible serialization tags for a [`Trie`] enum.
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive, ToPrimitive)]
#[repr(u8)]
pub(crate) enum TrieTag {
    /// Represents a tag for a [`Trie::Leaf`] variant.
    Leaf = 0,
    /// Represents a tag for a [`Trie::Node`] variant.
    Node = 1,
    /// Represents a tag for a [`Trie::Extension`] variant.
    Extension = 2,
}

impl From<TrieTag> for u8 {
    fn from(value: TrieTag) -> Self {
        TrieTag::to_u8(&value).unwrap() // SAFETY: TrieTag is represented as u8.
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
    fn tag(&self) -> TrieTag {
        match self {
            Trie::Leaf { .. } => TrieTag::Leaf,
            Trie::Node { .. } => TrieTag::Node,
            Trie::Extension { .. } => TrieTag::Extension,
        }
    }

    pub fn tag_type(&self) -> String {
        match self {
            Trie::Leaf { .. } => "Leaf".to_string(),
            Trie::Node { .. } => "Node".to_string(),
            Trie::Extension { .. } => "Extension".to_string(),
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
            .map(|bytes| Digest::hash_into_chunks_if_necessary(&bytes))
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
    pub fn iter_children(&self) -> DescendantsIterator {
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

pub(crate) type LazyTrieLeaf<K, V> = Either<Bytes, Trie<K, V>>;

pub(crate) fn lazy_trie_tag(bytes: &[u8]) -> Option<TrieTag> {
    bytes.first().copied().and_then(TrieTag::from_u8)
}

pub(crate) fn lazy_trie_deserialize<K, V>(
    bytes: Bytes,
) -> Result<LazyTrieLeaf<K, V>, bytesrepr::Error>
where
    K: FromBytes,
    V: FromBytes,
{
    let trie_tag = lazy_trie_tag(&bytes);

    if trie_tag == Some(TrieTag::Leaf) {
        Ok(Either::Left(bytes))
    } else {
        let deserialized: Trie<K, V> = bytesrepr::deserialize(bytes.into())?;
        Ok(Either::Right(deserialized))
    }
}

pub(crate) fn lazy_trie_iter_children<K, V>(
    trie_bytes: &LazyTrieLeaf<K, V>,
) -> DescendantsIterator {
    match trie_bytes {
        Either::Left(_) => {
            // Leaf bytes does not have any children
            DescendantsIterator::ZeroOrOne(None)
        }
        Either::Right(trie) => {
            // Trie::Node or Trie::Extension has children
            trie.iter_children()
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
        writer.push(u8::from(self.tag()));
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
    use casper_types::{
        bytesrepr::{self, ToBytes},
        Digest,
    };

    use crate::global_state::trie::Trie;

    /// Creates a tuple containing an empty root hash and an empty root (a node
    /// with an empty pointer block)
    pub fn create_hashed_empty_trie<K: ToBytes, V: ToBytes>(
    ) -> Result<(Digest, Trie<K, V>), bytesrepr::Error> {
        let root: Trie<K, V> = Trie::Node {
            pointer_block: Default::default(),
        };
        let root_bytes: Vec<u8> = root.to_bytes()?;
        Ok((Digest::hash(root_bytes), root))
    }
}
