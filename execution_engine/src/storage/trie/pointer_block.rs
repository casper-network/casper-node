use std::{
    convert::TryInto,
    fmt::{self, Formatter},
    mem::MaybeUninit,
};

use casper_hashing::Digest;
use casper_types::bytesrepr::{self, FromBytes, ToBytes, Writer, U8_SERIALIZED_LENGTH};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};

pub(crate) const USIZE_EXCEEDS_U8: &str = "usize exceeds u8";

pub(crate) const RADIX: usize = 256;

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
        U8_SERIALIZED_LENGTH + Digest::LENGTH
    }

    #[inline]
    fn write_bytes<W>(&self, writer: &mut W) -> Result<(), bytesrepr::Error>
    where
        W: Writer,
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
        for pointer in self.0.iter() {
            result.append(&mut pointer.to_bytes()?);
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.iter().map(ToBytes::serialized_length).sum()
    }

    fn write_bytes<W>(&self, writer: &mut W) -> Result<(), bytesrepr::Error>
    where
        W: Writer,
    {
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
}
