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

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

/// The number of bytes in an EVM 256-bit hash or word.
pub const HASH_LENGTH: usize = 32;

/// A 32-byte EVM hash or storage word.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct Hash(Digest);

impl Hash {
    /// The zero hash.
    pub const ZERO: Hash = Hash(Digest::from_raw([0; HASH_LENGTH]));

    /// Creates a hash from raw bytes.
    pub const fn new(bytes: [u8; HASH_LENGTH]) -> Self {
        Hash(Digest::from_raw(bytes))
    }

    /// Returns the raw bytes backing this hash.
    pub fn value(self) -> [u8; HASH_LENGTH] {
        self.0.value()
    }

    /// Returns the raw bytes backing this hash by reference.
    pub fn as_bytes(&self) -> &[u8; HASH_LENGTH] {
        <&[u8; HASH_LENGTH]>::try_from(self.0.as_ref()).expect("digest length is 32 bytes")
    }

    /// Returns `true` when all bytes are zero.
    pub fn is_zero(&self) -> bool {
        self.0.as_ref().iter().all(|byte| *byte == 0)
    }

    /// Returns a lower-case hexadecimal string without a `0x` prefix.
    pub fn to_hex_string(self) -> String {
        base16::encode_lower(&self.0)
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Display for Hash {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{}", self.to_hex_string())
    }
}

impl ToBytes for Hash {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }
}

impl FromBytes for Hash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Digest::from_bytes(bytes).map(|(digest, remainder)| (Hash(digest), remainder))
    }
}
