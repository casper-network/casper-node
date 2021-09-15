use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    safe_split_at,
};

use crate::Digest;

/// The number of bytes in a serialized `Blake2bHash`.
pub const BLAKE2B_HASH_SERIALIZED_LENGTH: usize = 32;

#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(
    feature = "std",
    derive(schemars::JsonSchema, serde::Serialize, serde::Deserialize,),
    serde(deny_unknown_fields)
)]
pub(super) struct Blake2bHash(pub(crate) [u8; Digest::LENGTH]);

impl AsRef<[u8]> for Blake2bHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for Blake2bHash {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        Ok(self.0.to_vec())
    }

    fn serialized_length(&self) -> usize {
        BLAKE2B_HASH_SERIALIZED_LENGTH
    }
}

impl FromBytes for Blake2bHash {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let mut result = [0u8; BLAKE2B_HASH_SERIALIZED_LENGTH];
        let (bytes, remainder) = safe_split_at(bytes, BLAKE2B_HASH_SERIALIZED_LENGTH)?;
        result.copy_from_slice(bytes);
        Ok((Blake2bHash(result), remainder))
    }
}

#[cfg(test)]
mod tests {
    use casper_types::bytesrepr::{FromBytes, ToBytes};

    use crate::{blake2b_hash::Blake2bHash, util::blake2b_hash};

    #[test]
    fn bytesrepr_serialization() {
        let original_hash = blake2b_hash("shrimp");
        let bytes = original_hash
            .to_bytes()
            .expect("should serialize correctly");

        let (deserialized_hash, remainder) =
            Blake2bHash::from_bytes(&bytes).expect("should deserialize correctly");

        assert_eq!(original_hash, deserialized_hash);
        assert!(remainder.is_empty());
    }

    #[test]
    fn bytesrepr_serialization_with_remainder() {
        let original_hash = blake2b_hash("shrimp");
        let mut bytes = original_hash
            .to_bytes()
            .expect("should serialize correctly");
        bytes.push(0xFF);

        let (deserialized_hash, remainder) =
            Blake2bHash::from_bytes(&bytes).expect("should deserialize correctly");

        assert_eq!(original_hash, deserialized_hash);
        assert_eq!(remainder.first().unwrap(), &0xFF);
        assert_eq!(remainder.len(), 1);
    }
}
