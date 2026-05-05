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

/// The number of bytes in an EVM 256-bit hash or word.
pub const HASH_LENGTH: usize = 32;

const HASH_SERIALIZED_LENGTH: usize = HASH_LENGTH;

/// A 32-byte EVM hash or storage word.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Hash([u8; HASH_LENGTH]);

impl Hash {
    /// The zero hash.
    pub const ZERO: Hash = Hash([0; HASH_LENGTH]);

    /// Creates a hash from raw bytes.
    pub const fn new(bytes: [u8; HASH_LENGTH]) -> Self {
        Hash(bytes)
    }

    /// Returns the raw bytes backing this hash.
    pub const fn value(self) -> [u8; HASH_LENGTH] {
        self.0
    }

    /// Returns the raw bytes backing this hash by reference.
    pub const fn as_bytes(&self) -> &[u8; HASH_LENGTH] {
        &self.0
    }

    /// Returns `true` when all bytes are zero.
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }

    /// Returns a lower-case hexadecimal string without a `0x` prefix.
    pub fn to_hex_string(self) -> String {
        base16::encode_lower(&self.0)
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for Hash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{}", self.to_hex_string())
    }
}

impl ToBytes for Hash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(self.0.to_vec())
    }

    fn serialized_length(&self) -> usize {
        HASH_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend_from_slice(&self.0);
        Ok(())
    }
}

impl FromBytes for Hash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        if bytes.len() < HASH_LENGTH {
            return Err(bytesrepr::Error::EarlyEndOfStream);
        }
        let (hash, remainder) = bytes.split_at(HASH_LENGTH);
        let hash = <[u8; HASH_LENGTH]>::try_from(hash).map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((Hash(hash), remainder))
    }
}
