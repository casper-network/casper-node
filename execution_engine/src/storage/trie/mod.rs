//! Core types for a Merkle Trie

use std::{
    convert::TryInto,
    fmt::{self, Debug, Display, Formatter},
    mem::MaybeUninit,
};

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::{FromPrimitive, ToPrimitive};
use serde::{
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::shared::newtypes::Blake2bHash;
use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Tagged,
};

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

/// Represents all possible serialized variants of a [`Pointer`].
#[derive(FromPrimitive, ToPrimitive)]
pub enum PointerTag {
    /// Leaf variant.
    LeafPointer = 0,
    /// Node variant.
    NodePointer = 1,
}

impl From<PointerTag> for u8 {
    fn from(tag: PointerTag) -> Self {
        tag.to_u8().expect("PointerTag is represented as u8")
    }
}

impl FromBytes for PointerTag {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag_value, rem) = FromBytes::from_bytes(bytes)?;
        let tag = PointerTag::from_u8(tag_value).ok_or(bytesrepr::Error::Formatting)?;
        Ok((tag, rem))
    }
}

/// Represents a pointer to the next object in a Merkle Trie
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pointer {
    /// Leaf pointer.
    LeafPointer(Blake2bHash),
    /// Node pointer.
    NodePointer(Blake2bHash),
}

impl Pointer {
    /// Borrows the inner hash from a `Pointer`.
    pub fn hash(&self) -> &Blake2bHash {
        match self {
            Pointer::LeafPointer(hash) => hash,
            Pointer::NodePointer(hash) => hash,
        }
    }

    /// Takes ownership of the hash, consuming this `Pointer`.
    pub fn into_hash(self) -> Blake2bHash {
        match self {
            Pointer::LeafPointer(hash) => hash,
            Pointer::NodePointer(hash) => hash,
        }
    }

    /// Creates a new owned `Pointer` with a new `Blake2bHash`.
    pub fn update(&self, hash: Blake2bHash) -> Self {
        match self {
            Pointer::LeafPointer(_) => Pointer::LeafPointer(hash),
            Pointer::NodePointer(_) => Pointer::NodePointer(hash),
        }
    }
}

impl Tagged for Pointer {
    type Tag = PointerTag;

    fn tag(&self) -> Self::Tag {
        match self {
            Pointer::LeafPointer(_) => PointerTag::LeafPointer,
            Pointer::NodePointer(_) => PointerTag::NodePointer,
        }
    }
}

impl ToBytes for Pointer {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.push(self.tag().into());
        ret.extend_from_slice(self.hash().as_ref());
        Ok(ret)
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH + Blake2bHash::LENGTH
    }
}

impl FromBytes for Pointer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, rem) = PointerTag::from_bytes(bytes)?;
        match tag {
            PointerTag::LeafPointer => {
                let (hash, rem) = Blake2bHash::from_bytes(rem)?;
                Ok((Pointer::LeafPointer(hash), rem))
            }
            PointerTag::NodePointer => {
                let (hash, rem) = Blake2bHash::from_bytes(rem)?;
                Ok((Pointer::NodePointer(hash), rem))
            }
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
        ret.push(self.tag());

        match self {
            Trie::Leaf { key, value } => {
                ret.append(&mut key.to_bytes()?);
                ret.append(&mut value.to_bytes()?);
            }
            Trie::Node { pointer_block } => {
                ret.append(&mut pointer_block.to_bytes()?);
            }
            Trie::Extension { affix, pointer } => {
                ret.append(&mut affix.to_bytes()?);
                ret.append(&mut pointer.to_bytes()?);
            }
        }
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

    use crate::{shared::newtypes::Blake2bHash, storage::trie::Trie};

    /// Creates a tuple containing an empty root hash and an empty root (a node
    /// with an empty pointer block)
    pub fn create_hashed_empty_trie<K: ToBytes, V: ToBytes>(
    ) -> Result<(Blake2bHash, Trie<K, V>), bytesrepr::Error> {
        let root: Trie<K, V> = Trie::Node {
            pointer_block: Default::default(),
        };
        let root_bytes: Vec<u8> = root.to_bytes()?;
        Ok((Blake2bHash::new(&root_bytes), root))
    }
}
