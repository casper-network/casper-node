use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

use bitflags::bitflags;
#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::bytesrepr;

/// The number of bytes in a serialized [`AccessRights`].
pub const ACCESS_RIGHTS_SERIALIZED_LENGTH: usize = 1;

bitflags! {
    /// A struct which behaves like a set of bitflags to define access rights associated with a
    /// [`URef`](crate::URef).
    #[allow(clippy::derive_hash_xor_eq)]
    #[cfg_attr(feature = "datasize", derive(DataSize))]
    pub struct AccessRights: u8 {
        /// No permissions
        const NONE = 0;
        /// Permission to read the value under the associated `URef`.
        const READ  = 0b001;
        /// Permission to write a value under the associated `URef`.
        const WRITE = 0b010;
        /// Permission to add to the value under the associated `URef`.
        const ADD   = 0b100;
        /// Permission to read or add to the value under the associated `URef`.
        const READ_ADD       = Self::READ.bits | Self::ADD.bits;
        /// Permission to read or write the value under the associated `URef`.
        const READ_WRITE     = Self::READ.bits | Self::WRITE.bits;
        /// Permission to add to, or write the value under the associated `URef`.
        const ADD_WRITE      = Self::ADD.bits  | Self::WRITE.bits;
        /// Permission to read, add to, or write the value under the associated `URef`.
        const READ_ADD_WRITE = Self::READ.bits | Self::ADD.bits | Self::WRITE.bits;
    }
}

impl Default for AccessRights {
    fn default() -> Self {
        AccessRights::NONE
    }
}

impl AccessRights {
    /// Returns `true` if the `READ` flag is set.
    pub fn is_readable(self) -> bool {
        self & AccessRights::READ == AccessRights::READ
    }

    /// Returns `true` if the `WRITE` flag is set.
    pub fn is_writeable(self) -> bool {
        self & AccessRights::WRITE == AccessRights::WRITE
    }

    /// Returns `true` if the `ADD` flag is set.
    pub fn is_addable(self) -> bool {
        self & AccessRights::ADD == AccessRights::ADD
    }

    /// Returns `true` if no flags are set.
    pub fn is_none(self) -> bool {
        self == AccessRights::NONE
    }
}

impl Display for AccessRights {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match *self {
            AccessRights::NONE => write!(f, "NONE"),
            AccessRights::READ => write!(f, "READ"),
            AccessRights::WRITE => write!(f, "WRITE"),
            AccessRights::ADD => write!(f, "ADD"),
            AccessRights::READ_ADD => write!(f, "READ_ADD"),
            AccessRights::READ_WRITE => write!(f, "READ_WRITE"),
            AccessRights::ADD_WRITE => write!(f, "ADD_WRITE"),
            AccessRights::READ_ADD_WRITE => write!(f, "READ_ADD_WRITE"),
            _ => write!(f, "UNKNOWN"),
        }
    }
}

impl bytesrepr::ToBytes for AccessRights {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.bits.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        ACCESS_RIGHTS_SERIALIZED_LENGTH
    }
}

impl bytesrepr::FromBytes for AccessRights {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (id, rem) = u8::from_bytes(bytes)?;
        match AccessRights::from_bits(id) {
            Some(rights) => Ok((rights, rem)),
            None => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Serialize for AccessRights {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.bits.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AccessRights {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u8::deserialize(deserializer)?;
        AccessRights::from_bits(bits).ok_or_else(|| SerdeError::custom("invalid bits"))
    }
}

impl Distribution<AccessRights> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> AccessRights {
        let mut result = AccessRights::NONE;
        if rng.gen() {
            result |= AccessRights::READ;
        }
        if rng.gen() {
            result |= AccessRights::WRITE;
        }
        if rng.gen() {
            result |= AccessRights::ADD;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_readable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_readable(), is_true)
    }

    #[test]
    fn test_is_readable() {
        test_readable(AccessRights::READ, true);
        test_readable(AccessRights::READ_ADD, true);
        test_readable(AccessRights::READ_WRITE, true);
        test_readable(AccessRights::READ_ADD_WRITE, true);
        test_readable(AccessRights::ADD, false);
        test_readable(AccessRights::ADD_WRITE, false);
        test_readable(AccessRights::WRITE, false);
    }

    fn test_writable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_writeable(), is_true)
    }

    #[test]
    fn test_is_writable() {
        test_writable(AccessRights::WRITE, true);
        test_writable(AccessRights::READ_WRITE, true);
        test_writable(AccessRights::ADD_WRITE, true);
        test_writable(AccessRights::READ, false);
        test_writable(AccessRights::ADD, false);
        test_writable(AccessRights::READ_ADD, false);
        test_writable(AccessRights::READ_ADD_WRITE, true);
    }

    fn test_addable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_addable(), is_true)
    }

    #[test]
    fn test_is_addable() {
        test_addable(AccessRights::ADD, true);
        test_addable(AccessRights::READ_ADD, true);
        test_addable(AccessRights::READ_WRITE, false);
        test_addable(AccessRights::ADD_WRITE, true);
        test_addable(AccessRights::READ, false);
        test_addable(AccessRights::WRITE, false);
        test_addable(AccessRights::READ_ADD_WRITE, true);
    }
}
