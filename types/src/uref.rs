use alloc::{format, string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

use hex_fmt::HexFmt;

use crate::{bytesrepr, AccessRights, ApiError, Key, ACCESS_RIGHTS_SERIALIZED_LENGTH};

/// The number of bytes in a [`URef`] address.
pub const UREF_ADDR_LENGTH: usize = 32;

/// The number of bytes in a serialized [`URef`] where the [`AccessRights`] are not `None`.
pub const UREF_SERIALIZED_LENGTH: usize = UREF_ADDR_LENGTH + ACCESS_RIGHTS_SERIALIZED_LENGTH;

/// The address of a [`URef`](types::URef) (unforgeable reference) on the network.
pub type URefAddr = [u8; UREF_ADDR_LENGTH];

/// Represents an unforgeable reference, containing an address in the network's global storage and
/// the [`AccessRights`] of the reference.
///
/// A `URef` can be used to index entities such as [`CLValue`](crate::CLValue)s, or smart contracts.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct URef(URefAddr, AccessRights);

impl URef {
    /// Constructs a [`URef`] from an address and access rights.
    pub fn new(address: URefAddr, access_rights: AccessRights) -> Self {
        URef(address, access_rights)
    }

    /// Returns the address of this [`URef`].
    pub fn addr(&self) -> URefAddr {
        self.0
    }

    /// Returns the access rights of this [`URef`].
    pub fn access_rights(&self) -> AccessRights {
        self.1
    }

    /// Returns a new [`URef`] with the same address and updated access rights.
    pub fn with_access_rights(self, access_rights: AccessRights) -> Self {
        URef(self.0, access_rights)
    }

    /// Removes the access rights from this [`URef`].
    pub fn remove_access_rights(self) -> Self {
        URef(self.0, AccessRights::NONE)
    }

    /// Returns `true` if the access rights are `Some` and
    /// [`is_readable`](AccessRights::is_readable) is `true` for them.
    pub fn is_readable(self) -> bool {
        self.1.is_readable()
    }

    /// Returns a new [`URef`] with the same address and [`AccessRights::READ`] permission.
    pub fn into_read(self) -> URef {
        URef(self.0, AccessRights::READ)
    }

    /// Returns a new [`URef`] with the same address and [`AccessRights::READ_ADD_WRITE`]
    /// permission.
    pub fn into_read_add_write(self) -> URef {
        URef(self.0, AccessRights::READ_ADD_WRITE)
    }

    /// Returns `true` if the access rights are `Some` and
    /// [`is_writeable`](AccessRights::is_writeable) is `true` for them.
    pub fn is_writeable(self) -> bool {
        self.1.is_writeable()
    }

    /// Returns `true` if the access rights are `Some` and [`is_addable`](AccessRights::is_addable)
    /// is `true` for them.
    pub fn is_addable(self) -> bool {
        self.1.is_addable()
    }

    /// Formats the address and access rights of the [`URef`] in an unique way that could be used as
    /// a name when storing the given `URef` in a global state.
    pub fn as_string(&self) -> String {
        // Extract bits as numerical value, with no flags marked as 0.
        let access_rights_bits = self.access_rights().bits();
        // Access rights is represented as octal, which means that max value of u8 can
        // be represented as maximum of 3 octal digits.
        format!(
            "uref-{}-{:03o}",
            base16::encode_lower(&self.addr()),
            access_rights_bits
        )
    }
}

impl Display for URef {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let addr = self.addr();
        let access_rights = self.access_rights();
        write!(f, "URef({}, {})", HexFmt(&addr), access_rights)
    }
}

impl Debug for URef {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl bytesrepr::ToBytes for URef {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        UREF_SERIALIZED_LENGTH
    }
}

impl bytesrepr::FromBytes for URef {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (id, rem): ([u8; 32], &[u8]) = bytesrepr::FromBytes::from_bytes(bytes)?;
        let (access_rights, rem): (AccessRights, &[u8]) = bytesrepr::FromBytes::from_bytes(rem)?;
        Ok((URef(id, access_rights), rem))
    }
}

impl TryFrom<Key> for URef {
    type Error = ApiError;

    fn try_from(key: Key) -> Result<Self, Self::Error> {
        if let Key::URef(uref) = key {
            Ok(uref)
        } else {
            Err(ApiError::UnexpectedKeyVariant)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uref_as_string() {
        // Since we are putting URefs to named_keys map keyed by the label that
        // `as_string()` returns, any changes to the string representation of
        // that type cannot break the format.
        let addr_array = [0u8; 32];
        let uref_a = URef::new(addr_array, AccessRights::READ);
        assert_eq!(
            uref_a.as_string(),
            "uref-0000000000000000000000000000000000000000000000000000000000000000-001"
        );
        let uref_b = URef::new(addr_array, AccessRights::WRITE);
        assert_eq!(
            uref_b.as_string(),
            "uref-0000000000000000000000000000000000000000000000000000000000000000-002"
        );

        let uref_c = uref_b.remove_access_rights();
        assert_eq!(
            uref_c.as_string(),
            "uref-0000000000000000000000000000000000000000000000000000000000000000-000"
        );
    }
}
