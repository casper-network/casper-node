mod legacy_pointer_block;

use bitvec::prelude::*;
use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{
    convert::TryInto,
    fmt::{self, Formatter},
};

pub(crate) use legacy_pointer_block::LegacyPointerBlock;

use crate::storage::trie::pointer_block::legacy_pointer_block::LegacyDigest;

use self::legacy_pointer_block::LegacyPointer;

pub(crate) const USIZE_EXCEEDS_U8: &str = "usize exceeds u8";

pub(crate) const RADIX: usize = 256;

const POINTER_SERIALIZED_LENGTH: usize = Digest::LENGTH + 1;

#[derive(FromPrimitive)]
enum PointerTag {
    Leaf = 0,
    Node = 1,
}

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
    fn tag(&self) -> PointerTag {
        match self {
            Pointer::LeafPointer(_) => PointerTag::Leaf,
            Pointer::NodePointer(_) => PointerTag::Node,
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
        POINTER_SERIALIZED_LENGTH
    }

    #[inline]
    fn write_bytes<W>(&self, writer: &mut W) -> Result<(), bytesrepr::Error>
    where
        W: bytesrepr::Writer,
    {
        writer.write_u8(self.tag() as u8)?;
        writer.write_bytes(self.hash())?;
        Ok(())
    }
}

impl FromBytes for Pointer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag_byte, rem) = u8::from_bytes(bytes)?;
        let tag = PointerTag::from_u8(tag_byte).ok_or(bytesrepr::Error::Formatting)?;
        match tag {
            PointerTag::Leaf => {
                let (hash, rem) = Digest::from_bytes(rem)?;
                Ok((Pointer::LeafPointer(hash), rem))
            }
            PointerTag::Node => {
                let (hash, rem) = Digest::from_bytes(rem)?;
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

impl From<LegacyPointerBlock> for PointerBlock {
    fn from(legacy_pointer_block: LegacyPointerBlock) -> Self {
        let mut pointer_block_array = [None; RADIX];
        for (legacy_index, legacy_pointer) in legacy_pointer_block.as_legacy_indexed_pointers() {
            let pointer = match legacy_pointer {
                LegacyPointer::LegacyLeafPointer(LegacyDigest(legacy_digest)) => {
                    Pointer::LeafPointer(Digest::from(*legacy_digest))
                }
                LegacyPointer::LegacyNodePointer(LegacyDigest(legacy_digest)) => {
                    Pointer::NodePointer(Digest::from(*legacy_digest))
                }
            };

            debug_assert!(legacy_index <= pointer_block_array.len());
            pointer_block_array[legacy_index] = Some(pointer);
        }
        PointerBlock(pointer_block_array)
    }
}

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
    pub fn as_indexed_pointers(&self) -> impl Iterator<Item = (u8, &Pointer)> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(index, maybe_pointer)| {
                maybe_pointer
                    .as_ref()
                    .map(|value| (index.try_into().expect(USIZE_EXCEEDS_U8), value))
            })
    }

    /// Gets the count of children for this `PointerBlock`.
    pub fn child_count(&self) -> usize {
        self.as_indexed_pointers().count()
    }

    /// Returns iterator over all pointers.
    pub fn iter(&self) -> impl Iterator<Item = &Option<Pointer>> {
        self.0.iter()
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
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        let cnt = self.child_count();
        (RADIX / 8) + ((cnt + 7) / 8) + (Digest::LENGTH * cnt)
    }

    fn write_bytes<W>(&self, writer: &mut W) -> Result<(), bytesrepr::Error>
    where
        W: bytesrepr::Writer,
    {
        let mut option_tags = [0u8; RADIX / 8];
        let mut variant_tags: BitVec<Msb0, _> = BitVec::new();

        let total_some_variants = {
            let option_tag_bits = option_tags.view_bits_mut::<Msb0>();

            for (index, pointer) in self.iter().enumerate() {
                let option_tag = match pointer {
                    Some(Pointer::LeafPointer(_)) => {
                        variant_tags.push(true);
                        true
                    }
                    Some(Pointer::NodePointer(_)) => {
                        variant_tags.push(false);
                        true
                    }
                    None => false,
                };
                option_tag_bits.set(index, option_tag);
            }

            option_tag_bits.count_ones()
        };

        writer.write_bytes(&option_tags)?;

        debug_assert_eq!(
            variant_tags.as_raw_slice().len(),
            (total_some_variants + 7) / 8
        );
        writer.write_bytes(&variant_tags.as_raw_slice())?;

        for (_index, pointer) in self.as_indexed_pointers() {
            pointer.hash().write_bytes(writer)?;
        }

        Ok(())
    }
}

impl FromBytes for PointerBlock {
    fn from_bytes(mut bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let mut pointer_block_array: PointerBlockArray = [None; RADIX];

        let option_tags = {
            let (option_tags_data, rem) = bytesrepr::safe_split_at(bytes, RADIX / 8)?;
            bytes = rem;

            option_tags_data.view_bits::<Msb0>()
        };

        let variant_tags = {
            let some_count = option_tags.count_ones();

            // We know that all variant tags will occupy exactly (N+7)/8 bytes.
            let (variant_tags, rem) = bytesrepr::safe_split_at(bytes, (some_count + 7) / 8)?;
            bytes = rem;

            variant_tags.view_bits::<Msb0>()
        };

        let mut variant_tags_iter = variant_tags.iter();

        for (position, option_tag) in option_tags.iter().enumerate() {
            if !*option_tag {
                // We don't care about 0s as we know that only 1s makes sense to check next bit in
                // the variant tags.
                continue;
            }

            let (digest, rem) = Digest::from_bytes(bytes)?;
            bytes = rem;

            let pointer = match variant_tags_iter.next() {
                Some(flag) if flag == true => Pointer::LeafPointer(digest),
                Some(flag) if flag == false => Pointer::NodePointer(digest),
                Some(_) | None => unreachable!(), // We asserted correct size earlier
            };

            pointer_block_array[position] = Some(pointer);
        }

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

#[cfg(test)]
mod tests {
    use casper_hashing::Digest;

    use super::*;

    #[test]
    fn radix_is_256() {
        assert_eq!(
            RADIX, 256,
            "Changing RADIX alone might cause things to break"
        );
    }

    /// A defense against changes to [`RADIX`](history::trie::RADIX).
    #[test]
    fn debug_formatter_succeeds() {
        let _ = format!("{:?}", PointerBlock::new());
    }

    #[test]
    fn assignment_and_indexing() {
        let test_hash = Digest::hash(b"TrieTrieAgain");
        let leaf_pointer = Some(Pointer::LeafPointer(test_hash));
        let mut pointer_block = PointerBlock::new();
        pointer_block[0] = leaf_pointer;
        pointer_block[RADIX - 1] = leaf_pointer;
        assert_eq!(leaf_pointer, pointer_block[0]);
        assert_eq!(leaf_pointer, pointer_block[RADIX - 1]);
        assert_eq!(None, pointer_block[1]);
        assert_eq!(None, pointer_block[RADIX - 2]);
    }

    #[test]
    #[should_panic]
    fn assignment_off_end() {
        let test_hash = Digest::hash(b"TrieTrieAgain");
        let leaf_pointer = Some(Pointer::LeafPointer(test_hash));
        let mut pointer_block = PointerBlock::new();
        pointer_block[RADIX] = leaf_pointer;
    }

    #[test]
    #[should_panic]
    fn indexing_off_end() {
        let pointer_block = PointerBlock::new();
        let _val = pointer_block[RADIX];
    }

    #[test]
    fn serialization_round_trip() {
        let mut pointer_block = PointerBlock::default();
        for a in 0..RADIX {
            match a % 3 {
                0 => continue,
                1 => {
                    pointer_block[a] = Some(Pointer::NodePointer(Digest::hash(
                        format!("{}", a).as_bytes(),
                    )))
                }
                2 => {
                    pointer_block[a] = Some(Pointer::LeafPointer(Digest::hash(
                        format!("{}", a).as_bytes(),
                    )))
                }
                _ => unreachable!(),
            }
        }
        bytesrepr::test_serialization_roundtrip(&pointer_block);

        let mut bytes = pointer_block.to_bytes().unwrap();
        bytes.push(255);

        let (pointer_block_2, rem) = PointerBlock::from_bytes(&bytes).unwrap();
        assert_eq!(rem, b"\xff");
        assert_eq!(pointer_block, pointer_block_2);
    }
}
