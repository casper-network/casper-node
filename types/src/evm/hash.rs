use alloc::{string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Digest,
};

/// The number of bytes in an EVM 256-bit hash or word.
pub const HASH_LENGTH: usize = 32;

/// A 32-byte EVM hash or storage word.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
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

#[cfg(feature = "json-schema")]
impl JsonSchema for Hash {
    fn schema_name() -> String {
        String::from("EvmHash")
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("A 32-byte EVM hash or storage word encoded as 0x-prefixed hexadecimal.".into());
        schema_object.into()
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.collect_str(self)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let value = String::deserialize(deserializer)?;
            let hex = value
                .strip_prefix("0x")
                .ok_or_else(|| D::Error::custom("hash must start with 0x"))?;
            let bytes = base16::decode(hex.as_bytes()).map_err(SerdeError::custom)?;
            let bytes =
                <[u8; HASH_LENGTH]>::try_from(bytes.as_ref()).map_err(SerdeError::custom)?;
            Ok(Hash::new(bytes))
        } else {
            Digest::deserialize(deserializer).map(Hash)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_readable_serde_uses_0x_prefixed_hex() {
        let hash = Hash::new([0xab; HASH_LENGTH]);
        let expected_hex = "ab".repeat(HASH_LENGTH);

        let encoded = serde_json::to_string(&hash).expect("hash should serialize");
        assert_eq!(encoded, format!("\"0x{expected_hex}\""));

        let decoded: Hash = serde_json::from_str(&encoded).expect("hash should deserialize");
        assert_eq!(decoded, hash);
    }

    #[test]
    fn non_human_readable_serde_roundtrip() {
        let hash = Hash::new([0xcd; HASH_LENGTH]);
        let encoded = bincode::serialize(&hash).expect("hash should serialize");
        let decoded: Hash = bincode::deserialize(&encoded).expect("hash should deserialize");

        assert_eq!(decoded, hash);
    }
}
