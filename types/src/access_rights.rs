use alloc::{collections::BTreeMap, vec::Vec};
use core::fmt::{self, Display, Formatter};

use bitflags::bitflags;
#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{bytesrepr, Key, URef, URefAddr};

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

/// Access rights for a given runtime context.
#[derive(Debug, PartialEq)]
pub struct ContextAccessRights {
    context_key: Key,
    access_rights: BTreeMap<URefAddr, AccessRights>,
}

impl ContextAccessRights {
    /// Creates a new instance of access rights.
    pub fn new(context_key: Key, access_rights: BTreeMap<URefAddr, AccessRights>) -> Self {
        ContextAccessRights {
            context_key,
            access_rights,
        }
    }

    /// Returns the current context key.
    pub fn context_key(&self) -> Key {
        self.context_key
    }

    /// Extends the current access rights from a given set of URefs.
    pub fn extend(&mut self, urefs: &[URef]) {
        for uref in urefs {
            let access_rights = self
                .access_rights
                .entry(uref.addr())
                .or_insert_with(AccessRights::default);
            *access_rights = access_rights.union(uref.access_rights());
        }
    }

    /// Merges two given sets of access rights which returns the union between the two.
    pub fn merge(&mut self, other: Self) {
        for (uref_addr, bits) in other.access_rights {
            let access_rights = self
                .access_rights
                .entry(uref_addr)
                .or_insert_with(AccessRights::default);
            *access_rights = access_rights.union(bits);
        }
    }

    /// Checks whether given uref has enough access rights.
    pub fn uref_has_access_rights(&self, uref: &URef) -> bool {
        if let Some(known_rights) = self.access_rights.get(&uref.addr()) {
            let rights_to_check = uref.access_rights();
            known_rights.contains(rights_to_check)
        } else {
            // URef is not known
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UREF_ADDR: [u8; 32] = [1u8; 32];

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

    #[test]
    fn should_check_uref_has_access_rights() {
        let uref = URef::new(UREF_ADDR, AccessRights::READ_ADD_WRITE);
        let mut access_rights = BTreeMap::new();
        access_rights.insert(UREF_ADDR, AccessRights::READ_ADD_WRITE);
        let context_rights = ContextAccessRights::new(Key::from(uref), access_rights);
        assert!(context_rights.uref_has_access_rights(&uref))
    }

    #[test]
    fn should_check_uref_does_not_have_access_rights() {
        let uref = URef::new(UREF_ADDR, AccessRights::READ_ADD_WRITE);
        let mut access_rights = BTreeMap::new();
        access_rights.insert(UREF_ADDR, AccessRights::READ_ADD);
        let context_rights = ContextAccessRights::new(Key::from(uref), access_rights);
        assert!(!context_rights.uref_has_access_rights(&uref))
    }

    #[test]
    fn should_perform_union_of_access_rights() {
        let uref = URef::new(UREF_ADDR, AccessRights::READ_ADD);
        let mut access_rights = BTreeMap::new();
        access_rights.insert(UREF_ADDR, AccessRights::READ_ADD_WRITE);
        let context_rights = ContextAccessRights::new(Key::from(uref), access_rights);
        assert!(context_rights.uref_has_access_rights(&uref))
    }
}
