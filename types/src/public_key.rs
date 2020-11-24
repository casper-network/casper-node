use alloc::{string::String, vec::Vec};
use core::{
    cmp,
    fmt::{self, Display, Formatter},
    iter,
};

use datasize::DataSize;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped,
};

const PUBLIC_KEY_VARIANT_LENGTH: usize = 1;
/// Length of a ED25519 PublicKey.
pub const ED25519_PUBLIC_KEY_LENGTH: usize = 32;
const ED25519_VARIANT_ID: u8 = 1;
const SECP256K1_PUBLIC_KEY_LENGTH: usize = 33;
const SECP256K1_VARIANT_ID: u8 = 2;

#[derive(Debug)]
pub enum FromStrError {
    InvalidPrefix(u8),
    Hex(base16::DecodeError),
    Length { expected: usize, found: usize },
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix(prefix) => {
                write!(f, "invalid public key prefix of {}", prefix)
            }
            FromStrError::Hex(error) => write!(f, "public key decode from hex: {}", error),
            FromStrError::Length { expected, found } => write!(
                f,
                "wrong length of public key - expected {}, found {}",
                expected, found
            ),
        }
    }
}

pub type Secp256k1BytesArray = [u8; SECP256K1_PUBLIC_KEY_LENGTH];

/// Represents the bytes of a secp256k1 public key.
#[derive(Copy, Clone, DataSize)]
pub struct Secp256k1Bytes(Secp256k1BytesArray);

impl Secp256k1Bytes {
    /// Returns the underlying bytes.
    pub fn value(self) -> Secp256k1BytesArray {
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

impl From<Secp256k1BytesArray> for Secp256k1Bytes {
    fn from(value: Secp256k1BytesArray) -> Self {
        Secp256k1Bytes(value)
    }
}

impl FromBytes for Secp256k1Bytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (secp256k1_bytes, rem) = FromBytes::from_bytes(bytes)?;
        Ok((Secp256k1Bytes(secp256k1_bytes), rem))
    }
}

impl ToBytes for Secp256k1Bytes {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        // Without prefix similar to [u8; N] arrays.
        self.0.to_bytes()
    }
    fn serialized_length(&self) -> usize {
        SECP256K1_PUBLIC_KEY_LENGTH
    }
}

/// Simplified raw data type
#[derive(DataSize, Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublicKey {
    /// Ed25519 public key.
    Ed25519([u8; ED25519_PUBLIC_KEY_LENGTH]),
    /// Secp256k1 public key.
    Secp256k1(Secp256k1Bytes),
}

impl PublicKey {
    /// Converts the public key to hex, where the first byte represents the algorithm tag.
    pub fn to_hex(&self) -> String {
        let bytes = iter::once(&self.variant_id())
            .chain(self.as_ref())
            .copied()
            .collect::<Vec<u8>>();
        base16::encode_lower(&bytes)
    }

    /// Tries to decode the public key from its hex-representation.  The hex format should be as
    /// produced by `PublicKey::to_hex()`.
    pub fn from_hex<T: AsRef<[u8]>>(input: T) -> Result<PublicKey, FromStrError> {
        if input.as_ref().len() < 2 {
            return Err(FromStrError::Length {
                expected: ED25519_PUBLIC_KEY_LENGTH,
                found: input.as_ref().len(),
            });
        }

        let (tag_bytes, key_bytes) = input.as_ref().split_at(2);
        let mut tag = [0u8; 1];
        base16::decode_slice(tag_bytes, tag.as_mut())?;

        let bytes = base16::decode(key_bytes)?;
        let public_key = match tag[0] {
            ED25519_VARIANT_ID => {
                if bytes.len() != ED25519_PUBLIC_KEY_LENGTH {
                    return Err(FromStrError::Length {
                        expected: ED25519_PUBLIC_KEY_LENGTH,
                        found: bytes.len(),
                    });
                }
                let mut array = [0; ED25519_PUBLIC_KEY_LENGTH];
                array.copy_from_slice(&bytes);
                PublicKey::Ed25519(array)
            }
            SECP256K1_VARIANT_ID => {
                if bytes.len() != SECP256K1_PUBLIC_KEY_LENGTH {
                    return Err(FromStrError::Length {
                        expected: SECP256K1_PUBLIC_KEY_LENGTH,
                        found: bytes.len(),
                    });
                }
                let mut array = [0; SECP256K1_PUBLIC_KEY_LENGTH];
                array.copy_from_slice(&bytes);
                PublicKey::Secp256k1(Secp256k1Bytes(array))
            }
            _ => return Err(FromStrError::InvalidPrefix(tag[0])),
        };
        Ok(public_key)
    }

    fn variant_id(&self) -> u8 {
        match self {
            PublicKey::Ed25519(_) => ED25519_VARIANT_ID,
            PublicKey::Secp256k1(_) => SECP256K1_VARIANT_ID,
        }
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        match self {
            PublicKey::Ed25519(bytes) => bytes.as_ref(),
            PublicKey::Secp256k1(bytes) => bytes.as_ref(),
        }
    }
}

impl ToBytes for PublicKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.variant_id().to_bytes()?);
        match self {
            PublicKey::Ed25519(bytes) => buffer.extend(bytes.to_bytes()?),
            PublicKey::Secp256k1(bytes) => buffer.extend(bytes.to_bytes()?),
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
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
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
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

/// Used to serialize and deserialize `PublicKey` where the (de)serializer is not a human-readable
/// type.
#[derive(Serialize, Deserialize)]
enum PublicKeyAsBytes<'a> {
    Ed25519(&'a [u8]),
    Secp256k1(&'a [u8]),
}

impl<'a> From<&'a PublicKey> for PublicKeyAsBytes<'a> {
    fn from(public_key: &'a PublicKey) -> Self {
        match public_key {
            PublicKey::Ed25519(ed25519) => PublicKeyAsBytes::Ed25519(ed25519.as_ref()),
            PublicKey::Secp256k1(secp256k1) => PublicKeyAsBytes::Secp256k1(secp256k1.as_ref()),
        }
    }
}

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_hex().serialize(serializer)
        } else {
            PublicKeyAsBytes::from(self).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let hex_string = String::deserialize(deserializer)?;
            return PublicKey::from_hex(hex_string.as_bytes()).map_err(D::Error::custom);
        }

        let as_bytes = PublicKeyAsBytes::deserialize(deserializer)?;
        match as_bytes {
            PublicKeyAsBytes::Ed25519(bytes) => {
                if bytes.len() != ED25519_PUBLIC_KEY_LENGTH {
                    return Err(D::Error::custom(FromStrError::Length {
                        expected: ED25519_PUBLIC_KEY_LENGTH,
                        found: bytes.len(),
                    }));
                }
                let mut array = [0; ED25519_PUBLIC_KEY_LENGTH];
                array.copy_from_slice(bytes);
                Ok(PublicKey::Ed25519(array))
            }
            PublicKeyAsBytes::Secp256k1(bytes) => {
                if bytes.len() != SECP256K1_PUBLIC_KEY_LENGTH {
                    return Err(D::Error::custom(FromStrError::Length {
                        expected: SECP256K1_PUBLIC_KEY_LENGTH,
                        found: bytes.len(),
                    }));
                }
                let mut array = [0; SECP256K1_PUBLIC_KEY_LENGTH];
                array.copy_from_slice(bytes);
                Ok(PublicKey::Secp256k1(Secp256k1Bytes(array)))
            }
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
    fn bytesrepr_roundtrip_ed25519() {
        let public_key = PublicKey::Ed25519([42; 32]);
        bytesrepr::test_serialization_roundtrip(&public_key);
    }

    #[test]
    fn bytesrepr_roundtrip_secp256k1() {
        let public_key = PublicKey::Secp256k1([43; 33].into());
        bytesrepr::test_serialization_roundtrip(&public_key);
    }

    #[test]
    fn bytesrepr_roundtrip_cl() {
        let public_key = PublicKey::Ed25519([42; 32]);
        let cl = CLValue::from_t(public_key).unwrap();
        bytesrepr::test_serialization_roundtrip(&cl);
    }

    #[test]
    fn bytesrepr_roundtrip_cl_secp256k1() {
        let public_key = PublicKey::Secp256k1([42; 33].into());
        let cl = CLValue::from_t(public_key).unwrap();
        bytesrepr::test_serialization_roundtrip(&cl);
    }

    #[test]
    fn serde_roundtrip_ed25519() {
        let public_key = PublicKey::Ed25519([42; 32]);
        let serialized = bincode::serialize(&public_key).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(public_key, decoded);
    }

    #[test]
    fn serde_roundtrip_secp256k1() {
        let public_key = PublicKey::Secp256k1([42; 33].into());
        let serialized = bincode::serialize(&public_key).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(public_key, decoded);
    }

    #[test]
    fn json_roundtrip_ed25519() {
        let public_key = PublicKey::Ed25519([42; 32]);
        let json_string = serde_json::to_string_pretty(&public_key).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(public_key, decoded);
    }

    #[test]
    fn json_roundtrip_secp256k1() {
        let public_key = PublicKey::Secp256k1([42; 33].into());
        let json_string = serde_json::to_string_pretty(&public_key).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(public_key, decoded);
    }

    #[test]
    fn to_hex_roundtrip_ed25519() {
        let public_key = PublicKey::Ed25519([42; 32]);
        let hex_string = public_key.to_hex();
        let decoded = PublicKey::from_hex(&hex_string).unwrap();
        assert_eq!(public_key, decoded);
    }

    #[test]
    fn to_hex_roundtrip_secp256k1() {
        let public_key = PublicKey::Secp256k1([42; 33].into());
        let hex_string = public_key.to_hex();
        let decoded = PublicKey::from_hex(&hex_string).unwrap();
        assert_eq!(public_key, decoded);
    }
}
