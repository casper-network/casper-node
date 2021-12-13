use std::{
    convert::TryInto,
    mem::{self, MaybeUninit},
};

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use casper_types::bytesrepr::{Error, FromBytes};

const LEGACY_NONE_TAG: u8 = 0;
const LEGACY_SOME_TAG: u8 = 1;

const LEGACY_RADIX: usize = 256;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyDigest(pub(crate) [u8; LegacyDigest::LENGTH]);

impl LegacyDigest {
    const LENGTH: usize = 32;
}

impl FromBytes for LegacyDigest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = if bytes.len() < LegacyDigest::LENGTH {
            return Err(Error::EarlyEndOfStream);
        } else {
            bytes.split_at(LegacyDigest::LENGTH)
        };
        debug_assert_eq!(bytes.len(), LegacyDigest::LENGTH);
        // SAFETY: We asserted length at the top.
        let array = bytes.try_into().unwrap();
        Ok((LegacyDigest(array), rem))
    }
}

#[derive(FromPrimitive)]
enum LegacyPointerTag {
    LegacyLeaf = 0,
    LegacyNode = 1,
}

/// Represents a simplified data layout for a legacy pointer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LegacyPointer {
    /// Leaf pointer.
    LegacyLeafPointer(LegacyDigest),
    /// Node pointer.
    LegacyNodePointer(LegacyDigest),
}

impl FromBytes for LegacyPointer {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag_byte, rem) = u8::from_bytes(bytes)?;
        let tag = LegacyPointerTag::from_u8(tag_byte).ok_or(Error::Formatting)?;
        match tag {
            LegacyPointerTag::LegacyLeaf => {
                let (legacy_hash, rem) = LegacyDigest::from_bytes(rem)?;
                Ok((LegacyPointer::LegacyLeafPointer(legacy_hash), rem))
            }
            LegacyPointerTag::LegacyNode => {
                let (legacy_hash, rem) = LegacyDigest::from_bytes(rem)?;
                Ok((LegacyPointer::LegacyNodePointer(legacy_hash), rem))
            }
        }
    }
}

type LegacyPointerBlockValue = Option<LegacyPointer>;

type LegacyPointerBlockArray = [LegacyPointerBlockValue; LEGACY_RADIX];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyPointerBlock(LegacyPointerBlockArray);

impl LegacyPointerBlock {
    pub(crate) fn as_legacy_indexed_pointers(
        &self,
    ) -> impl Iterator<Item = (usize, &LegacyPointer)> {
        self.0
            .iter()
            .enumerate()
            .filter_map(|(index, maybe_pointer)| {
                debug_assert!(index <= u8::MAX as usize);
                maybe_pointer.as_ref().map(|value| (index, value))
            })
    }
}

impl FromBytes for LegacyPointerBlock {
    fn from_bytes(mut bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        // This way we can safely use MaybeUninit without dropping on error.
        assert!(!mem::needs_drop::<LegacyPointerBlockArray>());

        let pointer_block_array = {
            // With MaybeUninit here we can avoid default initialization of result array below.
            let mut result: MaybeUninit<LegacyPointerBlockArray> = MaybeUninit::uninit();
            let result_ptr = result.as_mut_ptr() as *mut LegacyPointerBlockValue;
            for i in 0..LEGACY_RADIX {
                let (option_tag, rem) = u8::from_bytes(bytes)?;
                bytes = rem;
                let legacy_pointer = match option_tag {
                    LEGACY_NONE_TAG => None,
                    LEGACY_SOME_TAG => {
                        let (t, rem) = LegacyPointer::from_bytes(bytes)?;
                        bytes = rem;
                        Some(t)
                    }
                    _ => return Err(Error::Formatting),
                };
                unsafe { result_ptr.add(i).write(legacy_pointer) }
            }
            unsafe { result.assume_init() }
        };
        Ok((LegacyPointerBlock(pointer_block_array), bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_legacy_digest(i: usize) -> LegacyDigest {
        let i_bytes = i.to_be_bytes();
        let mut digest = [0; 32];
        digest[32 - i_bytes.len()..].copy_from_slice(&i_bytes);
        LegacyDigest(digest)
    }

    fn make_legacy_pointer_block() -> LegacyPointerBlock {
        let mut legacy_pointer_block_array: LegacyPointerBlockArray = [None; LEGACY_RADIX];

        for (i, maybe_pointer) in legacy_pointer_block_array.iter_mut().enumerate() {
            //0..LEGACY_RADIX {
            let legacy_pointer = match i % 3 {
                0 => {
                    *maybe_pointer = None;
                    continue;
                }
                1 => {
                    let legacy_digest = make_legacy_digest(i);
                    LegacyPointer::LegacyLeafPointer(legacy_digest)
                }
                2 => {
                    let legacy_digest = make_legacy_digest(i);
                    LegacyPointer::LegacyNodePointer(legacy_digest)
                }
                _ => unreachable!(),
            };
            *maybe_pointer = Some(legacy_pointer);
        }

        LegacyPointerBlock(legacy_pointer_block_array)
    }

    fn serialize_legacy_pointer(legacy_pointer: &LegacyPointer) -> Vec<u8> {
        let mut result = Vec::new();
        let (tag, legacy_digest) = match legacy_pointer {
            LegacyPointer::LegacyLeafPointer(digest) => (LegacyPointerTag::LegacyLeaf, digest),
            LegacyPointer::LegacyNodePointer(digest) => (LegacyPointerTag::LegacyNode, digest),
        };
        result.push(tag as u8);
        result.extend_from_slice(&legacy_digest.0);
        result
    }

    fn serialize_legacy_pointer_block(legacy_pointer_block: &LegacyPointerBlock) -> Vec<u8> {
        let mut result = Vec::new();
        for i in 0..LEGACY_RADIX {
            let maybe_pointer = legacy_pointer_block.0.get(i).unwrap();
            match maybe_pointer {
                Some(legacy_pointer) => {
                    result.push(LEGACY_SOME_TAG);
                    result.extend_from_slice(&serialize_legacy_pointer(legacy_pointer));
                }
                None => {
                    result.push(LEGACY_NONE_TAG);
                }
            }
        }
        result
    }

    #[test]
    fn should_not_deserialize_invalid_length() {
        // LEGACY_SOME_TAG .. LEGACY_NODE_POINTER_TAG .. 0x00
        assert_eq!(
            LegacyPointerBlock::from_bytes(b"\x01\x01\x00").unwrap_err(),
            Error::EarlyEndOfStream
        );
    }

    #[test]
    fn should_deserialize_sparse_pointer_blocks() {
        let legacy_pointer_block = make_legacy_pointer_block();

        let serialized = serialize_legacy_pointer_block(&legacy_pointer_block);

        LegacyPointerBlock::from_bytes(&serialized[0..serialized.len() / 2]).unwrap_err();

        let (deserialized, rem) = LegacyPointerBlock::from_bytes(&serialized).unwrap();
        assert_eq!(deserialized, legacy_pointer_block);
        assert!(rem.is_empty());

        let mut serialized_extra = serialized.clone();
        serialized_extra.push(255);
        let (deserialized_2, rem) = LegacyPointerBlock::from_bytes(&serialized_extra).unwrap();
        assert_eq!(deserialized_2, legacy_pointer_block);
        assert_eq!(rem, b"\xff");
    }
}
