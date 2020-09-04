use alloc::vec::Vec;
use core::{cmp, fmt};

use crate::{
    bytesrepr::{self, ToBytes},
    CLType, CLTyped,
};
use bytesrepr::{Error, FromBytes};
use serde::{Deserialize, Serialize};

const PUBLIC_KEY_VARIANT_LENGTH: usize = 1;
const ED25519_PUBLIC_KEY_LENGTH: usize = 32;
const ED25519_VARIANT_ID: u8 = 1;
const SECP256K1_PUBLIC_KEY_LENGTH: usize = 33;
const SECP256K1_VARIANT_ID: u8 = 2;

// This is inside a private module so that the generated `BigArray` does not form part of this
// crate's public API, and hence also doesn't appear in the rustdocs.
mod big_array {
    use serde_big_array::big_array;

    big_array! { BigArray; super::SECP256K1_PUBLIC_KEY_LENGTH }
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Secp256k1Bytes(#[serde(with = "big_array::BigArray")] [u8; SECP256K1_PUBLIC_KEY_LENGTH]);

impl Secp256k1Bytes {
    pub fn value(self) -> [u8; SECP256K1_PUBLIC_KEY_LENGTH] {
        self.0
    }
}

impl Ord for Secp256k1Bytes {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.0.as_ref().cmp(other.0.as_ref())
    }
}

impl PartialOrd for Secp256k1Bytes {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Secp256k1Bytes {}

impl PartialEq for Secp256k1Bytes {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_ref().eq(other.0.as_ref())
    }
}

impl fmt::Debug for Secp256k1Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0.as_ref())
    }
}

impl AsRef<[u8]> for Secp256k1Bytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<[u8; SECP256K1_PUBLIC_KEY_LENGTH]> for Secp256k1Bytes {
    fn from(value: [u8; SECP256K1_PUBLIC_KEY_LENGTH]) -> Self {
        Secp256k1Bytes(value)
    }
}

impl FromBytes for Secp256k1Bytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = bytesrepr::safe_split_at(bytes, SECP256K1_PUBLIC_KEY_LENGTH)?;
        let mut result = [0u8; SECP256K1_PUBLIC_KEY_LENGTH];
        result.copy_from_slice(bytes);
        Ok((Secp256k1Bytes(result), rem))
    }
}

impl ToBytes for Secp256k1Bytes {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        // Without prefix similar to [u8; N] arrays.
        Ok(self.0.to_vec())
    }
    fn serialized_length(&self) -> usize {
        SECP256K1_PUBLIC_KEY_LENGTH
    }
}

/// Simplified raw data type
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PublicKey {
    /// Ed25519 public key.
    Ed25519([u8; ED25519_PUBLIC_KEY_LENGTH]),
    /// Secp256k1 public key.
    Secp256k1(Secp256k1Bytes),
}

impl PublicKey {
    fn variant_id(&self) -> u8 {
        match self {
            PublicKey::Ed25519(_) => ED25519_VARIANT_ID,
            PublicKey::Secp256k1(_) => SECP256K1_VARIANT_ID,
        }
    }
}

impl ToBytes for PublicKey {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.variant_id().to_bytes()?);
        match self {
            PublicKey::Ed25519(bytes) => buffer.extend((*bytes).to_bytes()?),
            PublicKey::Secp256k1(bytes) => buffer.extend((*bytes).to_bytes()?),
        }
        Ok(buffer)
    }
    fn serialized_length(&self) -> usize {
        PUBLIC_KEY_VARIANT_LENGTH
            + match self {
                PublicKey::Ed25519(bytes) => bytes.serialized_length(),
                PublicKey::Secp256k1(bytes) => bytes.serialized_length(),
            }
    }
}

impl FromBytes for PublicKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (variant_id, bytes): (u8, _) = FromBytes::from_bytes(bytes)?;
        match variant_id {
            ED25519_VARIANT_ID => {
                let (ed25519_bytes, bytes) = FromBytes::from_bytes(bytes)?;
                Ok((PublicKey::Ed25519(ed25519_bytes), bytes))
            }
            SECP256K1_VARIANT_ID => {
                let (secp256k1_bytes, bytes) = FromBytes::from_bytes(bytes)?;
                Ok((PublicKey::Secp256k1(secp256k1_bytes), bytes))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl CLTyped for PublicKey {
    fn cl_type() -> CLType {
        CLType::PublicKey
    }
}

#[cfg(test)]
mod tests {
    use super::PublicKey;
    use crate::{bytesrepr, CLValue};

    #[test]
    fn serialization_roundtrip_ed25519() {
        let public_key = PublicKey::Ed25519([42; 32]);
        bytesrepr::test_serialization_roundtrip(&public_key);
    }

    #[test]
    fn serialization_roundtrip_secp256k1() {
        let public_key = PublicKey::Secp256k1([43; 33].into());
        bytesrepr::test_serialization_roundtrip(&public_key);
    }

    #[test]
    fn serialization_roundtrip_cl() {
        let public_key = PublicKey::Ed25519([42; 32]);
        let cl = CLValue::from_t(public_key).unwrap();
        bytesrepr::test_serialization_roundtrip(&cl);
    }
    #[test]
    fn serialization_roundtrip_cl_secp256k1() {
        let public_key = PublicKey::Secp256k1([42; 33].into());
        let cl = CLValue::from_t(public_key).unwrap();
        bytesrepr::test_serialization_roundtrip(&cl);
    }
}
