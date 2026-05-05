use alloc::{string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// The number of bytes in an EVM address.
pub const ADDRESS_LENGTH: usize = 20;

const ADDRESS_SERIALIZED_LENGTH: usize = ADDRESS_LENGTH;

/// A 20-byte Ethereum account or contract address.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Address([u8; ADDRESS_LENGTH]);

impl Address {
    /// The zero EVM address.
    pub const ZERO: Address = Address([0; ADDRESS_LENGTH]);

    /// Creates an address from raw bytes.
    pub const fn new(bytes: [u8; ADDRESS_LENGTH]) -> Self {
        Address(bytes)
    }

    /// Returns the raw bytes backing this address.
    pub const fn value(self) -> [u8; ADDRESS_LENGTH] {
        self.0
    }

    /// Returns the raw bytes backing this address by reference.
    pub const fn as_bytes(&self) -> &[u8; ADDRESS_LENGTH] {
        &self.0
    }

    /// Returns a lower-case hexadecimal string without a `0x` prefix.
    pub fn to_hex_string(self) -> String {
        base16::encode_lower(&self.0)
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for Address {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{}", self.to_hex_string())
    }
}

impl ToBytes for Address {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(self.0.to_vec())
    }

    fn serialized_length(&self) -> usize {
        ADDRESS_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend_from_slice(&self.0);
        Ok(())
    }
}

impl FromBytes for Address {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        if bytes.len() < ADDRESS_LENGTH {
            return Err(bytesrepr::Error::EarlyEndOfStream);
        }
        let (address, remainder) = bytes.split_at(ADDRESS_LENGTH);
        let address =
            <[u8; ADDRESS_LENGTH]>::try_from(address).map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((Address(address), remainder))
    }
}
