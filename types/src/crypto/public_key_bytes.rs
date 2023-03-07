use core::{
    convert::TryInto,
    fmt::{self, Display, Formatter},
};

use alloc::{string::String, vec::Vec};
use hex_fmt::HexFmt;
use serde::{de, Deserialize, Deserializer, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    checksummed_hex, PublicKey,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};

const SYSTEM_LENGTH: usize = 0;

/// The length in bytes of an Ed25519 public key.
const ED25519_LENGTH: usize = 32;

/// The length in bytes of a secp256k1 public key.
const SECP256K1_LENGTH: usize = 33;

const SYSTEM_TAG: u8 = 0;
const ED25519_TAG: u8 = 1;
const SECP256K1_TAG: u8 = 2;
const SYSTEM: &str = "System";
const ED25519: &str = "Ed25519";
const SECP256K1: &str = "Secp256k1";

/// A public asymmetric key.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum PublicKeyBytes {
    /// System public key.
    System,
    /// Ed25519 public key.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Ed25519([u8; ED25519_LENGTH]),
    /// secp256k1 public key.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Secp256k1([u8; SECP256K1_LENGTH]),
}

#[derive(Debug)]
pub enum Error {
    /// Error resulting from creating or using a public key types.
    AsymmetricKey(String),

    /// Error resulting when decoding a type from a hex-encoded representation.
    FromHex(base16::DecodeError),

    /// Error resulting when decoding a type from a base64 representation.
    FromBase64(base64::DecodeError),
}

impl From<base64::DecodeError> for Error {
    fn from(v: base64::DecodeError) -> Self {
        Self::FromBase64(v)
    }
}

impl From<base16::DecodeError> for Error {
    fn from(v: base16::DecodeError) -> Self {
        Self::FromHex(v)
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::AsymmetricKey(other) => write!(formatter, "{}", other),
            Error::FromHex(error) => write!(formatter, "base16 decoding error: {}", error),
            Error::FromBase64(error) => write!(formatter, "base64 decoding error: {}", error),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum AsymmetricTypeAsBytes {
    System,
    Ed25519(Vec<u8>),
    Secp256k1(Vec<u8>),
}

impl From<&PublicKeyBytes> for AsymmetricTypeAsBytes {
    fn from(public_key_bytes: &PublicKeyBytes) -> Self {
        match public_key_bytes {
            PublicKeyBytes::System => Self::System,
            PublicKeyBytes::Ed25519(ed25519_bytes) => Self::Ed25519(ed25519_bytes.to_vec()),
            PublicKeyBytes::Secp256k1(secp256k1_bytes) => Self::Secp256k1(secp256k1_bytes.to_vec()),
        }
    }
}

impl Serialize for PublicKeyBytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        {
            if serializer.is_human_readable() {
                return self.to_hex().serialize(serializer);
            }

            AsymmetricTypeAsBytes::from(self).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for PublicKeyBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let hex_string = String::deserialize(deserializer)?;
            let value =
                PublicKeyBytes::from_hex(hex_string.as_bytes()).map_err(de::Error::custom)?;
            return Ok(value);
        }

        let as_bytes = AsymmetricTypeAsBytes::deserialize(deserializer)?;
        match as_bytes {
            AsymmetricTypeAsBytes::System => Ok(Self::System),
            AsymmetricTypeAsBytes::Ed25519(raw_bytes) => {
                Self::ed25519_from_bytes(&raw_bytes).map_err(de::Error::custom)
            }
            AsymmetricTypeAsBytes::Secp256k1(raw_bytes) => {
                Self::secp256k1_from_bytes(&raw_bytes).map_err(de::Error::custom)
            }
        }
    }
}

impl PublicKeyBytes {
    fn tag(&self) -> u8 {
        match self {
            PublicKeyBytes::System => SYSTEM_TAG,
            PublicKeyBytes::Ed25519(_) => ED25519_TAG,
            PublicKeyBytes::Secp256k1(_) => SECP256K1_TAG,
        }
    }

    fn to_hex(&self) -> String {
        let mut bytes = vec![self.tag()];
        bytes.extend_from_slice(self.as_bytes());
        base16::encode_lower(&bytes)
    }

    fn variant_name(&self) -> &str {
        match self {
            PublicKeyBytes::System => SYSTEM,
            PublicKeyBytes::Ed25519(_) => ED25519,
            PublicKeyBytes::Secp256k1(_) => SECP256K1,
        }
    }
    fn as_bytes(&self) -> &[u8] {
        match self {
            PublicKeyBytes::System => &[],
            PublicKeyBytes::Ed25519(ed25519) => ed25519.as_slice(),
            PublicKeyBytes::Secp256k1(secp256k1) => secp256k1.as_slice(),
        }
    }

    fn ed25519_from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let ed25519_raw_bytes = bytes.try_into().map_err(|_| {
            Error::AsymmetricKey(format!(
                "invalid bytes length {} expected {} bytes",
                bytes.len(),
                ED25519_LENGTH
            ))
        })?;

        Ok(Self::Ed25519(ed25519_raw_bytes))
    }

    fn secp256k1_from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let secp256k1_raw_bytes = bytes.try_into().map_err(|_| {
            Error::AsymmetricKey(format!(
                "invalid bytes length {} expected {} bytes",
                bytes.len(),
                SECP256K1_LENGTH
            ))
        })?;

        Ok(Self::Secp256k1(secp256k1_raw_bytes))
    }

    /// Tries to decode `Self` from its hex-representation.  The hex format should be as produced
    /// by `AsymmetricType::to_hex()`.
    fn from_hex<A: AsRef<[u8]>>(input: A) -> Result<Self, Error> {
        if input.as_ref().len() < 2 {
            return Err(Error::AsymmetricKey("too short".into()));
        }

        let (tag_bytes, key_bytes) = input.as_ref().split_at(2);

        let tag = checksummed_hex::decode(&tag_bytes)?;
        let key_bytes = checksummed_hex::decode(&key_bytes)?;

        match tag[0] {
            ED25519_TAG => Self::ed25519_from_bytes(&key_bytes),
            SECP256K1_TAG => Self::secp256k1_from_bytes(&key_bytes),
            _ => Err(Error::AsymmetricKey(format!(
                "invalid tag.  Expected {} or {}, got {}",
                ED25519_TAG, SECP256K1_TAG, tag[0]
            ))),
        }
    }
}

impl ToBytes for PublicKeyBytes {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            PublicKeyBytes::System => {
                buffer.insert(0, SYSTEM_TAG);
            }
            PublicKeyBytes::Ed25519(public_key) => {
                buffer.insert(0, ED25519_TAG);
                let ed25519_bytes = public_key;
                buffer.extend_from_slice(ed25519_bytes);
            }
            PublicKeyBytes::Secp256k1(public_key) => {
                buffer.insert(0, SECP256K1_TAG);
                let secp256k1_bytes = public_key;
                buffer.extend_from_slice(secp256k1_bytes);
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        1 + match self {
            PublicKeyBytes::System => SYSTEM_LENGTH,
            PublicKeyBytes::Ed25519(_) => ED25519_LENGTH,
            PublicKeyBytes::Secp256k1(_) => SECP256K1_LENGTH,
        }
    }
}

impl FromBytes for PublicKeyBytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SYSTEM_TAG => Ok((PublicKeyBytes::System, remainder)),
            ED25519_TAG => {
                let (raw_bytes, remainder): ([u8; ED25519_LENGTH], _) =
                    FromBytes::from_bytes(remainder)?;
                let public_key = PublicKeyBytes::Ed25519(raw_bytes);
                Ok((public_key, remainder))
            }
            SECP256K1_TAG => {
                let (raw_bytes, remainder): ([u8; SECP256K1_LENGTH], _) =
                    FromBytes::from_bytes(remainder)?;
                let public_key = PublicKeyBytes::Secp256k1(raw_bytes);
                Ok((public_key, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl fmt::Debug for PublicKeyBytes {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PublicKey::{}({})",
            self.variant_name(),
            base16::encode_lower(self.as_bytes())
        )
    }
}

impl fmt::Display for PublicKeyBytes {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PubKey::{}({:10})",
            self.variant_name(),
            HexFmt(self.as_bytes())
        )
    }
}

impl From<&PublicKey> for PublicKeyBytes {
    fn from(public_key: &PublicKey) -> Self {
        match public_key {
            PublicKey::System => Self::System,
            PublicKey::Ed25519(ed25519_public_key) => {
                let ed25519_bytes = ed25519_public_key.as_bytes();
                Self::Ed25519(
                    ed25519_bytes
                        .as_slice()
                        .try_into()
                        .expect("lengths should match"),
                )
            }

            PublicKey::Secp256k1(secp256k1_bytes) => {
                let secp256k1_bytes = secp256k1_bytes.to_bytes();
                Self::Secp256k1(secp256k1_bytes)
            }
        }
    }
}

impl From<PublicKey> for PublicKeyBytes {
    fn from(public_key: PublicKey) -> Self {
        Self::from(&public_key)
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for PublicKeyBytes {
    fn schema_name() -> String {
        String::from("PublicKey")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some(
            "Hex-encoded cryptographic public key, including the algorithm tag prefix.".to_string(),
        );
        schema_object.into()
    }
}

#[cfg(test)]
mod tests {
    use core::convert::TryFrom;

    use crate::{PublicKey, PublicKeyBytes, SecretKey};

    #[test]
    fn should_convert_from_asymmetric_and_back() {
        let secret_key_1 =
            SecretKey::secp256k1_from_bytes(&[100; SecretKey::SECP256K1_LENGTH]).expect("ok");
        let secret_key_2 =
            SecretKey::ed25519_from_bytes(&[200; SecretKey::ED25519_LENGTH]).expect("ok");

        let public_key_1 = PublicKey::from(&secret_key_1);
        let public_key_2 = PublicKey::from(&secret_key_2);

        let public_key_bytes_1 = PublicKeyBytes::from(public_key_1.clone());
        let public_key_bytes_2 = PublicKeyBytes::from(public_key_2.clone());

        let public_key_1_from = PublicKey::try_from(public_key_bytes_1)
            .expect("key bytes created from a valid public key should be valid");
        let public_key_2_from = PublicKey::try_from(public_key_bytes_2)
            .expect("key bytes created from a valid public key should be valid");

        assert_eq!(public_key_1_from, public_key_1);
        assert_eq!(public_key_2_from, public_key_2);
    }

    #[test]
    fn should_serialize_to_json() {
        let secret_key_1 =
            SecretKey::secp256k1_from_bytes(&[100; SecretKey::SECP256K1_LENGTH]).expect("ok");
        let secret_key_2 =
            SecretKey::ed25519_from_bytes(&[200; SecretKey::ED25519_LENGTH]).expect("ok");

        let public_key_1 = PublicKey::from(&secret_key_1);
        let public_key_2 = PublicKey::from(&secret_key_2);

        let json_1 = serde_json::to_string(&public_key_1).unwrap();
        let public_key_bytes_1: PublicKeyBytes = serde_json::from_str(&json_1).unwrap();

        let public_key_bytes_1_json = serde_json::to_string(&public_key_bytes_1).unwrap();
        let public_key_1_deser: PublicKey = serde_json::from_str(&public_key_bytes_1_json).unwrap();
        assert_eq!(public_key_1, public_key_1_deser);

        let json_2 = serde_json::to_string(&public_key_2).unwrap();
        let public_key_bytes_2: PublicKeyBytes = serde_json::from_str(&json_2).unwrap();

        let public_key_bytes_2_json = serde_json::to_string(&public_key_bytes_2).unwrap();
        let public_key_2_deser: PublicKey = serde_json::from_str(&public_key_bytes_2_json).unwrap();
        assert_eq!(public_key_2, public_key_2_deser);
    }

    #[test]
    fn should_serialize_to_bincode() {
        let secret_key_1 =
            SecretKey::secp256k1_from_bytes(&[100; SecretKey::SECP256K1_LENGTH]).expect("ok");
        let secret_key_2 =
            SecretKey::ed25519_from_bytes(&[200; SecretKey::ED25519_LENGTH]).expect("ok");

        let public_key_1 = PublicKey::from(&secret_key_1);
        let public_key_2 = PublicKey::from(&secret_key_2);

        let json_1 = bincode::serialize(&public_key_1).unwrap();
        let public_key_bytes_1: PublicKeyBytes = bincode::deserialize(&json_1).unwrap();

        let public_key_bytes_1_json = serde_json::to_string(&public_key_bytes_1).unwrap();
        let public_key_1_deser: PublicKey = serde_json::from_str(&public_key_bytes_1_json).unwrap();
        assert_eq!(public_key_1, public_key_1_deser);

        let json_2 = bincode::serialize(&public_key_2).unwrap();
        let public_key_bytes_2: PublicKeyBytes = bincode::deserialize(&json_2).unwrap();

        let public_key_bytes_2_json = bincode::serialize(&public_key_bytes_2).unwrap();
        let public_key_2_deser: PublicKey = bincode::deserialize(&public_key_bytes_2_json).unwrap();
        assert_eq!(public_key_2, public_key_2_deser);
    }
}
