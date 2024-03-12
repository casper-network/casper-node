use core::fmt::Debug;

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    Digest,
};

#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;

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

    /// Returns a random `Pointer`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use rand::Rng;

        match rng.gen_range(0..2) {
            0 => Pointer::LeafPointer(Digest::random(rng)),
            1 => Pointer::NodePointer(Digest::random(rng)),
            _ => unreachable!(),
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
