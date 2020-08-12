use alloc::vec::Vec;

use crate::{
    bytesrepr::{self, ToBytes},
    CLType, CLTyped,
};
use bytesrepr::{Error, FromBytes};

const PUBLIC_KEY_VARIANT_LENGTH: usize = 1;
const ED25519_PUBLIC_KEY_LENGTH: usize = 32;
const SYSTEM_VARIANT_ID: u8 = 0;
const ED25519_VARIANT_ID: u8 = 1;

/// Simplified raw data type
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicKey {
    /// System account's public key.
    System,
    /// Ed25519 public key.
    Ed25519([u8; ED25519_PUBLIC_KEY_LENGTH]),
}

impl PublicKey {
    fn variant_id(&self) -> u8 {
        match self {
            PublicKey::Ed25519(_) => ED25519_VARIANT_ID,
            PublicKey::System => SYSTEM_VARIANT_ID,
        }
    }
}

impl ToBytes for PublicKey {
    fn to_bytes(&self) -> Result<Vec<u8>, crate::bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.variant_id().to_bytes()?);
        match self {
            PublicKey::Ed25519(bytes) => buffer.extend((*bytes).to_bytes()?),
            PublicKey::System => buffer.extend(&[0; 32]),
        }
        Ok(buffer)
    }
    fn serialized_length(&self) -> usize {
        PUBLIC_KEY_VARIANT_LENGTH + ED25519_PUBLIC_KEY_LENGTH
    }
}

impl FromBytes for PublicKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (variant_id, bytes) = FromBytes::from_bytes(bytes)?;
        match variant_id {
            ED25519_VARIANT_ID => {
                let (ed25519_bytes, bytes) = FromBytes::from_bytes(bytes)?;
                Ok((PublicKey::Ed25519(ed25519_bytes), bytes))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl CLTyped for PublicKey {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

#[cfg(test)]
mod tests {
    use super::PublicKey;
    use crate::bytesrepr;

    #[test]
    fn serialization_roundtrip() {
        let public_key = PublicKey::Ed25519([42; 32]);
        bytesrepr::test_serialization_roundtrip(&public_key);
    }
}
