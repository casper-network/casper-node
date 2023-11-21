//! Asymmetric key types and methods on them

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{
    cmp::Ordering,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    iter,
    marker::Copy,
};
#[cfg(any(feature = "std", test))]
use std::path::Path;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "std", test))]
use derp::{Der, Tag};
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SecretKey,
    VerifyingKey as Ed25519PublicKey, PUBLIC_KEY_LENGTH as ED25519_PUBLIC_KEY_LENGTH,
    SECRET_KEY_LENGTH as ED25519_SECRET_KEY_LENGTH, SIGNATURE_LENGTH as ED25519_SIGNATURE_LENGTH,
};
use hex_fmt::HexFmt;
use k256::ecdsa::{
    signature::{Signer, Verifier},
    Signature as Secp256k1Signature, SigningKey as Secp256k1SecretKey,
    VerifyingKey as Secp256k1PublicKey,
};
#[cfg(any(feature = "std", test))]
use once_cell::sync::Lazy;
#[cfg(any(feature = "std", test))]
use pem::Pem;
#[cfg(any(all(feature = "std", feature = "testing"), test))]
use rand::{Rng, RngCore};
#[cfg(feature = "json-schema")]
use schemars::{gen::SchemaGenerator, schema::Schema, JsonSchema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "json-schema")]
use serde_json::json;
#[cfg(any(feature = "std", test))]
use untrusted::Input;

#[cfg(any(all(feature = "std", feature = "testing"), test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex,
    crypto::Error,
    CLType, CLTyped, Tagged,
};
#[cfg(any(feature = "std", test))]
use crate::{
    crypto::ErrorExt,
    file_utils::{read_file, write_file, write_private_file},
};

#[cfg(any(feature = "testing", test))]
pub mod gens;
#[cfg(test)]
mod tests;

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

/// Tag for system variant.
pub const SYSTEM_TAG: u8 = 0;
const SYSTEM: &str = "System";

/// Tag for ed25519 variant.
pub const ED25519_TAG: u8 = 1;
const ED25519: &str = "Ed25519";

/// Tag for secp256k1 variant.
pub const SECP256K1_TAG: u8 = 2;
const SECP256K1: &str = "Secp256k1";

const SECP256K1_SECRET_KEY_LENGTH: usize = 32;
const SECP256K1_COMPRESSED_PUBLIC_KEY_LENGTH: usize = 33;
const SECP256K1_SIGNATURE_LENGTH: usize = 64;

/// Public key for system account.
pub const SYSTEM_ACCOUNT: PublicKey = PublicKey::System;

// See https://www.secg.org/sec1-v2.pdf#subsection.C.4
#[cfg(any(feature = "std", test))]
const EC_PUBLIC_KEY_OBJECT_IDENTIFIER: [u8; 7] = [42, 134, 72, 206, 61, 2, 1];

// See https://tools.ietf.org/html/rfc8410#section-10.3
#[cfg(any(feature = "std", test))]
const ED25519_OBJECT_IDENTIFIER: [u8; 3] = [43, 101, 112];
#[cfg(any(feature = "std", test))]
const ED25519_PEM_SECRET_KEY_TAG: &str = "PRIVATE KEY";
#[cfg(any(feature = "std", test))]
const ED25519_PEM_PUBLIC_KEY_TAG: &str = "PUBLIC KEY";

// Ref?
#[cfg(any(feature = "std", test))]
const SECP256K1_OBJECT_IDENTIFIER: [u8; 5] = [43, 129, 4, 0, 10];
#[cfg(any(feature = "std", test))]
const SECP256K1_PEM_SECRET_KEY_TAG: &str = "EC PRIVATE KEY";
#[cfg(any(feature = "std", test))]
const SECP256K1_PEM_PUBLIC_KEY_TAG: &str = "PUBLIC KEY";

#[cfg(any(feature = "std", test))]
static ED25519_SECRET_KEY: Lazy<SecretKey> = Lazy::new(|| {
    let bytes = [15u8; SecretKey::ED25519_LENGTH];
    SecretKey::ed25519_from_bytes(bytes).unwrap()
});

#[cfg(any(feature = "std", test))]
static ED25519_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let bytes = [15u8; SecretKey::ED25519_LENGTH];
    let secret_key = SecretKey::ed25519_from_bytes(bytes).unwrap();
    PublicKey::from(&secret_key)
});

/// Operations on asymmetric cryptographic type.
pub trait AsymmetricType<'a>
where
    Self: 'a + Sized + Tagged<u8>,
    Vec<u8>: From<&'a Self>,
{
    /// Converts `self` to hex, where the first byte represents the algorithm tag.
    fn to_hex(&'a self) -> String {
        let bytes = iter::once(self.tag())
            .chain(Vec::<u8>::from(self))
            .collect::<Vec<u8>>();
        base16::encode_lower(&bytes)
    }

    /// Tries to decode `Self` from its hex-representation.  The hex format should be as produced
    /// by `AsymmetricType::to_hex()`.
    fn from_hex<A: AsRef<[u8]>>(input: A) -> Result<Self, Error> {
        if input.as_ref().len() < 2 {
            return Err(Error::AsymmetricKey(
                "failed to decode from hex: too short".to_string(),
            ));
        }

        let (tag_hex, key_hex) = input.as_ref().split_at(2);

        let tag = checksummed_hex::decode(tag_hex)?;
        let key_bytes = checksummed_hex::decode(key_hex)?;

        match tag[0] {
            SYSTEM_TAG => {
                if key_bytes.is_empty() {
                    Ok(Self::system())
                } else {
                    Err(Error::AsymmetricKey(
                        "failed to decode from hex: invalid system variant".to_string(),
                    ))
                }
            }
            ED25519_TAG => Self::ed25519_from_bytes(&key_bytes),
            SECP256K1_TAG => Self::secp256k1_from_bytes(&key_bytes),
            _ => Err(Error::AsymmetricKey(format!(
                "failed to decode from hex: invalid tag.  Expected {}, {} or {}, got {}",
                SYSTEM_TAG, ED25519_TAG, SECP256K1_TAG, tag[0]
            ))),
        }
    }

    /// Constructs a new system variant.
    fn system() -> Self;

    /// Constructs a new ed25519 variant from a byte slice.
    fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error>;

    /// Constructs a new secp256k1 variant from a byte slice.
    fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error>;
}

/// A secret or private asymmetric key.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum SecretKey {
    /// System secret key.
    System,
    /// Ed25519 secret key.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    // Manually verified to have no data on the heap.
    Ed25519(Ed25519SecretKey),
    /// secp256k1 secret key.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Secp256k1(Secp256k1SecretKey),
}

impl SecretKey {
    /// The length in bytes of a system secret key.
    pub const SYSTEM_LENGTH: usize = 0;

    /// The length in bytes of an Ed25519 secret key.
    pub const ED25519_LENGTH: usize = ED25519_SECRET_KEY_LENGTH;

    /// The length in bytes of a secp256k1 secret key.
    pub const SECP256K1_LENGTH: usize = SECP256K1_SECRET_KEY_LENGTH;

    /// Constructs a new system variant.
    pub fn system() -> Self {
        SecretKey::System
    }

    /// Constructs a new ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error> {
        Ok(SecretKey::Ed25519(Ed25519SecretKey::try_from(
            bytes.as_ref(),
        )?))
    }

    /// Constructs a new secp256k1 variant from a byte slice.
    pub fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error> {
        Ok(SecretKey::Secp256k1(
            Secp256k1SecretKey::from_slice(bytes.as_ref()).map_err(|_| Error::SignatureError)?,
        ))
    }

    fn variant_name(&self) -> &str {
        match self {
            SecretKey::System => SYSTEM,
            SecretKey::Ed25519(_) => ED25519,
            SecretKey::Secp256k1(_) => SECP256K1,
        }
    }
}

#[cfg(any(feature = "std", test))]
impl SecretKey {
    /// Generates a new ed25519 variant using the system's secure random number generator.
    pub fn generate_ed25519() -> Result<Self, ErrorExt> {
        let mut bytes = [0u8; Self::ED25519_LENGTH];
        getrandom::getrandom(&mut bytes[..])?;
        SecretKey::ed25519_from_bytes(bytes).map_err(Into::into)
    }

    /// Generates a new secp256k1 variant using the system's secure random number generator.
    pub fn generate_secp256k1() -> Result<Self, ErrorExt> {
        let mut bytes = [0u8; Self::SECP256K1_LENGTH];
        getrandom::getrandom(&mut bytes[..])?;
        SecretKey::secp256k1_from_bytes(bytes).map_err(Into::into)
    }

    /// Attempts to write the key bytes to the configured file path.
    pub fn to_file<P: AsRef<Path>>(&self, file: P) -> Result<(), ErrorExt> {
        write_private_file(file, self.to_pem()?).map_err(ErrorExt::SecretKeySave)
    }

    /// Attempts to read the key bytes from configured file path.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self, ErrorExt> {
        let data = read_file(file).map_err(ErrorExt::SecretKeyLoad)?;
        Self::from_pem(data)
    }

    /// DER encodes a key.
    pub fn to_der(&self) -> Result<Vec<u8>, ErrorExt> {
        match self {
            SecretKey::System => Err(Error::System(String::from("to_der")).into()),
            SecretKey::Ed25519(secret_key) => {
                // See https://tools.ietf.org/html/rfc8410#section-10.3
                let mut key_bytes = vec![];
                let mut der = Der::new(&mut key_bytes);
                der.octet_string(&secret_key.to_bytes())?;

                let mut encoded = vec![];
                der = Der::new(&mut encoded);
                der.sequence(|der| {
                    der.integer(&[0])?;
                    der.sequence(|der| der.oid(&ED25519_OBJECT_IDENTIFIER))?;
                    der.octet_string(&key_bytes)
                })?;
                Ok(encoded)
            }
            SecretKey::Secp256k1(secret_key) => {
                // See https://www.secg.org/sec1-v2.pdf#subsection.C.4
                let mut oid_bytes = vec![];
                let mut der = Der::new(&mut oid_bytes);
                der.oid(&SECP256K1_OBJECT_IDENTIFIER)?;

                let mut encoded = vec![];
                der = Der::new(&mut encoded);
                der.sequence(|der| {
                    der.integer(&[1])?;
                    der.octet_string(secret_key.to_bytes().as_slice())?;
                    der.element(Tag::ContextSpecificConstructed0, &oid_bytes)
                })?;
                Ok(encoded)
            }
        }
    }

    /// Decodes a key from a DER-encoded slice.
    pub fn from_der<T: AsRef<[u8]>>(input: T) -> Result<Self, ErrorExt> {
        let input = Input::from(input.as_ref());

        let (key_type_tag, raw_bytes) = input.read_all(derp::Error::Read, |input| {
            derp::nested(input, Tag::Sequence, |input| {
                // Safe to ignore the first value which should be an integer.
                let version_slice =
                    derp::expect_tag_and_get_value(input, Tag::Integer)?.as_slice_less_safe();
                if version_slice.len() != 1 {
                    return Err(derp::Error::NonZeroUnusedBits);
                }
                let version = version_slice[0];

                // Read the next value.
                let (tag, value) = derp::read_tag_and_get_value(input)?;
                if tag == Tag::Sequence as u8 {
                    // Expecting an Ed25519 key.
                    if version != 0 {
                        return Err(derp::Error::WrongValue);
                    }

                    // The sequence should have one element: an object identifier defining Ed25519.
                    let object_identifier = value.read_all(derp::Error::Read, |input| {
                        derp::expect_tag_and_get_value(input, Tag::Oid)
                    })?;
                    if object_identifier.as_slice_less_safe() != ED25519_OBJECT_IDENTIFIER {
                        return Err(derp::Error::WrongValue);
                    }

                    // The third and final value should be the raw bytes of the secret key as an
                    // octet string in an octet string.
                    let raw_bytes = derp::nested(input, Tag::OctetString, |input| {
                        derp::expect_tag_and_get_value(input, Tag::OctetString)
                    })?
                    .as_slice_less_safe();

                    return Ok((ED25519_TAG, raw_bytes));
                } else if tag == Tag::OctetString as u8 {
                    // Expecting a secp256k1 key.
                    if version != 1 {
                        return Err(derp::Error::WrongValue);
                    }

                    // The octet string is the secret key.
                    let raw_bytes = value.as_slice_less_safe();

                    // The object identifier is next.
                    let parameter0 =
                        derp::expect_tag_and_get_value(input, Tag::ContextSpecificConstructed0)?;
                    let object_identifier = parameter0.read_all(derp::Error::Read, |input| {
                        derp::expect_tag_and_get_value(input, Tag::Oid)
                    })?;
                    if object_identifier.as_slice_less_safe() != SECP256K1_OBJECT_IDENTIFIER {
                        return Err(derp::Error::WrongValue);
                    }

                    // There might be an optional public key as the final value, but we're not
                    // interested in parsing that.  Read it to ensure `input.read_all` doesn't fail
                    // with unused bytes error.
                    let _ = derp::read_tag_and_get_value(input);

                    return Ok((SECP256K1_TAG, raw_bytes));
                }

                Err(derp::Error::WrongValue)
            })
        })?;

        match key_type_tag {
            SYSTEM_TAG => Err(Error::AsymmetricKey("cannot construct variant".to_string()).into()),
            ED25519_TAG => SecretKey::ed25519_from_bytes(raw_bytes).map_err(Into::into),
            SECP256K1_TAG => SecretKey::secp256k1_from_bytes(raw_bytes).map_err(Into::into),
            _ => Err(Error::AsymmetricKey("unknown type tag".to_string()).into()),
        }
    }

    /// PEM encodes a key.
    pub fn to_pem(&self) -> Result<String, ErrorExt> {
        let tag = match self {
            SecretKey::System => return Err(Error::System(String::from("to_pem")).into()),
            SecretKey::Ed25519(_) => ED25519_PEM_SECRET_KEY_TAG.to_string(),
            SecretKey::Secp256k1(_) => SECP256K1_PEM_SECRET_KEY_TAG.to_string(),
        };
        let contents = self.to_der()?;
        let pem = Pem { tag, contents };
        Ok(pem::encode(&pem))
    }

    /// Decodes a key from a PEM-encoded slice.
    pub fn from_pem<T: AsRef<[u8]>>(input: T) -> Result<Self, ErrorExt> {
        let pem = pem::parse(input)?;

        let secret_key = Self::from_der(&pem.contents)?;

        let bad_tag = |expected_tag: &str| {
            ErrorExt::FromPem(format!(
                "invalid tag: expected {}, got {}",
                expected_tag, pem.tag
            ))
        };

        match secret_key {
            SecretKey::System => return Err(Error::System(String::from("from_pem")).into()),
            SecretKey::Ed25519(_) => {
                if pem.tag != ED25519_PEM_SECRET_KEY_TAG {
                    return Err(bad_tag(ED25519_PEM_SECRET_KEY_TAG));
                }
            }
            SecretKey::Secp256k1(_) => {
                if pem.tag != SECP256K1_PEM_SECRET_KEY_TAG {
                    return Err(bad_tag(SECP256K1_PEM_SECRET_KEY_TAG));
                }
            }
        }

        Ok(secret_key)
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            Self::random_ed25519(rng)
        } else {
            Self::random_secp256k1(rng)
        }
    }

    /// Generates a random ed25519 instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_ed25519(rng: &mut TestRng) -> Self {
        let mut bytes = [0u8; Self::ED25519_LENGTH];
        rng.fill_bytes(&mut bytes[..]);
        SecretKey::ed25519_from_bytes(bytes).unwrap()
    }

    /// Generates a random secp256k1 instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_secp256k1(rng: &mut TestRng) -> Self {
        let mut bytes = [0u8; Self::SECP256K1_LENGTH];
        rng.fill_bytes(&mut bytes[..]);
        SecretKey::secp256k1_from_bytes(bytes).unwrap()
    }

    /// Returns an example value for documentation purposes.
    pub fn doc_example() -> &'static Self {
        &ED25519_SECRET_KEY
    }
}

impl Debug for SecretKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "SecretKey::{}", self.variant_name())
    }
}

impl Display for SecretKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        <Self as Debug>::fmt(self, formatter)
    }
}

impl Tagged<u8> for SecretKey {
    fn tag(&self) -> u8 {
        match self {
            SecretKey::System => SYSTEM_TAG,
            SecretKey::Ed25519(_) => ED25519_TAG,
            SecretKey::Secp256k1(_) => SECP256K1_TAG,
        }
    }
}

/// A public asymmetric key.
#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum PublicKey {
    /// System public key.
    System,
    /// Ed25519 public key.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Ed25519(Ed25519PublicKey),
    /// secp256k1 public key.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Secp256k1(Secp256k1PublicKey),
}

impl PublicKey {
    /// The length in bytes of a system public key.
    pub const SYSTEM_LENGTH: usize = 0;

    /// The length in bytes of an Ed25519 public key.
    pub const ED25519_LENGTH: usize = ED25519_PUBLIC_KEY_LENGTH;

    /// The length in bytes of a secp256k1 public key.
    pub const SECP256K1_LENGTH: usize = SECP256K1_COMPRESSED_PUBLIC_KEY_LENGTH;

    /// Creates an `AccountHash` from a given `PublicKey` instance.
    pub fn to_account_hash(&self) -> AccountHash {
        AccountHash::from(self)
    }

    /// Returns `true` if this public key is of the `System` variant.
    pub fn is_system(&self) -> bool {
        matches!(self, PublicKey::System)
    }

    fn variant_name(&self) -> &str {
        match self {
            PublicKey::System => SYSTEM,
            PublicKey::Ed25519(_) => ED25519,
            PublicKey::Secp256k1(_) => SECP256K1,
        }
    }
}

#[cfg(any(feature = "std", test))]
impl PublicKey {
    /// Generates a new ed25519 variant using the system's secure random number generator.
    pub fn generate_ed25519() -> Result<Self, ErrorExt> {
        let mut bytes = [0u8; Self::ED25519_LENGTH];
        getrandom::getrandom(&mut bytes[..]).expect("RNG failure!");
        PublicKey::ed25519_from_bytes(bytes).map_err(Into::into)
    }

    /// Generates a new secp256k1 variant using the system's secure random number generator.
    pub fn generate_secp256k1() -> Result<Self, ErrorExt> {
        let mut bytes = [0u8; Self::SECP256K1_LENGTH];
        getrandom::getrandom(&mut bytes[..]).expect("RNG failure!");
        PublicKey::secp256k1_from_bytes(bytes).map_err(Into::into)
    }

    /// Attempts to write the key bytes to the configured file path.
    pub fn to_file<P: AsRef<Path>>(&self, file: P) -> Result<(), ErrorExt> {
        write_file(file, self.to_pem()?).map_err(ErrorExt::PublicKeySave)
    }

    /// Attempts to read the key bytes from configured file path.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self, ErrorExt> {
        let data = read_file(file).map_err(ErrorExt::PublicKeyLoad)?;
        Self::from_pem(data)
    }

    /// DER encodes a key.
    pub fn to_der(&self) -> Result<Vec<u8>, ErrorExt> {
        match self {
            PublicKey::System => Err(Error::System(String::from("to_der")).into()),
            PublicKey::Ed25519(public_key) => {
                // See https://tools.ietf.org/html/rfc8410#section-10.1
                let mut encoded = vec![];
                let mut der = Der::new(&mut encoded);
                der.sequence(|der| {
                    der.sequence(|der| der.oid(&ED25519_OBJECT_IDENTIFIER))?;
                    der.bit_string(0, public_key.as_ref())
                })?;
                Ok(encoded)
            }
            PublicKey::Secp256k1(public_key) => {
                // See https://www.secg.org/sec1-v2.pdf#subsection.C.3
                let mut encoded = vec![];
                let mut der = Der::new(&mut encoded);
                der.sequence(|der| {
                    der.sequence(|der| {
                        der.oid(&EC_PUBLIC_KEY_OBJECT_IDENTIFIER)?;
                        der.oid(&SECP256K1_OBJECT_IDENTIFIER)
                    })?;
                    der.bit_string(0, public_key.to_encoded_point(true).as_ref())
                })?;
                Ok(encoded)
            }
        }
    }

    /// Decodes a key from a DER-encoded slice.
    pub fn from_der<T: AsRef<[u8]>>(input: T) -> Result<Self, ErrorExt> {
        let input = Input::from(input.as_ref());

        let mut key_type_tag = ED25519_TAG;
        let raw_bytes = input.read_all(derp::Error::Read, |input| {
            derp::nested(input, Tag::Sequence, |input| {
                derp::nested(input, Tag::Sequence, |input| {
                    // Read the first value.
                    let object_identifier =
                        derp::expect_tag_and_get_value(input, Tag::Oid)?.as_slice_less_safe();
                    if object_identifier == ED25519_OBJECT_IDENTIFIER {
                        key_type_tag = ED25519_TAG;
                        Ok(())
                    } else if object_identifier == EC_PUBLIC_KEY_OBJECT_IDENTIFIER {
                        // Assert the next object identifier is the secp256k1 ID.
                        let next_object_identifier =
                            derp::expect_tag_and_get_value(input, Tag::Oid)?.as_slice_less_safe();
                        if next_object_identifier != SECP256K1_OBJECT_IDENTIFIER {
                            return Err(derp::Error::WrongValue);
                        }

                        key_type_tag = SECP256K1_TAG;
                        Ok(())
                    } else {
                        Err(derp::Error::WrongValue)
                    }
                })?;
                Ok(derp::bit_string_with_no_unused_bits(input)?.as_slice_less_safe())
            })
        })?;

        match key_type_tag {
            ED25519_TAG => PublicKey::ed25519_from_bytes(raw_bytes).map_err(Into::into),
            SECP256K1_TAG => PublicKey::secp256k1_from_bytes(raw_bytes).map_err(Into::into),
            _ => unreachable!(),
        }
    }

    /// PEM encodes a key.
    pub fn to_pem(&self) -> Result<String, ErrorExt> {
        let tag = match self {
            PublicKey::System => return Err(Error::System(String::from("to_pem")).into()),
            PublicKey::Ed25519(_) => ED25519_PEM_PUBLIC_KEY_TAG.to_string(),
            PublicKey::Secp256k1(_) => SECP256K1_PEM_PUBLIC_KEY_TAG.to_string(),
        };
        let contents = self.to_der()?;
        let pem = Pem { tag, contents };
        Ok(pem::encode(&pem))
    }

    /// Decodes a key from a PEM-encoded slice.
    pub fn from_pem<T: AsRef<[u8]>>(input: T) -> Result<Self, ErrorExt> {
        let pem = pem::parse(input)?;
        let public_key = Self::from_der(&pem.contents)?;
        let bad_tag = |expected_tag: &str| {
            ErrorExt::FromPem(format!(
                "invalid tag: expected {}, got {}",
                expected_tag, pem.tag
            ))
        };
        match public_key {
            PublicKey::System => return Err(Error::System(String::from("from_pem")).into()),
            PublicKey::Ed25519(_) => {
                if pem.tag != ED25519_PEM_PUBLIC_KEY_TAG {
                    return Err(bad_tag(ED25519_PEM_PUBLIC_KEY_TAG));
                }
            }
            PublicKey::Secp256k1(_) => {
                if pem.tag != SECP256K1_PEM_PUBLIC_KEY_TAG {
                    return Err(bad_tag(SECP256K1_PEM_PUBLIC_KEY_TAG));
                }
            }
        }
        Ok(public_key)
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        PublicKey::from(&secret_key)
    }

    /// Generates a random ed25519 instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_ed25519(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random_ed25519(rng);
        PublicKey::from(&secret_key)
    }

    /// Generates a random secp256k1 instance using a `TestRng`.
    #[cfg(any(feature = "testing", test))]
    pub fn random_secp256k1(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random_secp256k1(rng);
        PublicKey::from(&secret_key)
    }

    /// Returns an example value for documentation purposes.
    pub fn doc_example() -> &'static Self {
        &ED25519_PUBLIC_KEY
    }
}

impl AsymmetricType<'_> for PublicKey {
    fn system() -> Self {
        PublicKey::System
    }

    fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error> {
        Ok(PublicKey::Ed25519(Ed25519PublicKey::try_from(
            bytes.as_ref(),
        )?))
    }

    fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error> {
        Ok(PublicKey::Secp256k1(
            Secp256k1PublicKey::from_sec1_bytes(bytes.as_ref())
                .map_err(|_| Error::SignatureError)?,
        ))
    }
}

impl From<&SecretKey> for PublicKey {
    fn from(secret_key: &SecretKey) -> PublicKey {
        match secret_key {
            SecretKey::System => PublicKey::System,
            SecretKey::Ed25519(secret_key) => PublicKey::Ed25519(secret_key.into()),
            SecretKey::Secp256k1(secret_key) => PublicKey::Secp256k1(secret_key.into()),
        }
    }
}

impl From<&PublicKey> for Vec<u8> {
    fn from(public_key: &PublicKey) -> Self {
        match public_key {
            PublicKey::System => Vec::new(),
            PublicKey::Ed25519(key) => key.to_bytes().into(),
            PublicKey::Secp256k1(key) => key.to_encoded_point(true).as_ref().into(),
        }
    }
}

impl From<PublicKey> for Vec<u8> {
    fn from(public_key: PublicKey) -> Self {
        Vec::<u8>::from(&public_key)
    }
}

impl Debug for PublicKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PublicKey::{}({})",
            self.variant_name(),
            base16::encode_lower(&Into::<Vec<u8>>::into(self))
        )
    }
}

impl Display for PublicKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PubKey::{}({:10})",
            self.variant_name(),
            HexFmt(Into::<Vec<u8>>::into(self))
        )
    }
}

impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PublicKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_tag = self.tag();
        let other_tag = other.tag();
        if self_tag == other_tag {
            Into::<Vec<u8>>::into(self).cmp(&Into::<Vec<u8>>::into(other))
        } else {
            self_tag.cmp(&other_tag)
        }
    }
}

// This implementation of `Hash` agrees with the derived `PartialEq`.  It's required since
// `ed25519_dalek::PublicKey` doesn't implement `Hash`.
#[allow(clippy::derived_hash_with_manual_eq)]
impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tag().hash(state);
        Into::<Vec<u8>>::into(self).hash(state);
    }
}

impl Tagged<u8> for PublicKey {
    fn tag(&self) -> u8 {
        match self {
            PublicKey::System => SYSTEM_TAG,
            PublicKey::Ed25519(_) => ED25519_TAG,
            PublicKey::Secp256k1(_) => SECP256K1_TAG,
        }
    }
}

impl ToBytes for PublicKey {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                PublicKey::System => Self::SYSTEM_LENGTH,
                PublicKey::Ed25519(_) => Self::ED25519_LENGTH,
                PublicKey::Secp256k1(_) => Self::SECP256K1_LENGTH,
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PublicKey::System => writer.push(SYSTEM_TAG),
            PublicKey::Ed25519(public_key) => {
                writer.push(ED25519_TAG);
                writer.extend_from_slice(public_key.as_bytes());
            }
            PublicKey::Secp256k1(public_key) => {
                writer.push(SECP256K1_TAG);
                writer.extend_from_slice(public_key.to_encoded_point(true).as_ref());
            }
        }
        Ok(())
    }
}

impl FromBytes for PublicKey {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SYSTEM_TAG => Ok((PublicKey::System, remainder)),
            ED25519_TAG => {
                let (raw_bytes, remainder): ([u8; Self::ED25519_LENGTH], _) =
                    FromBytes::from_bytes(remainder)?;
                let public_key = Self::ed25519_from_bytes(raw_bytes)
                    .map_err(|_error| bytesrepr::Error::Formatting)?;
                Ok((public_key, remainder))
            }
            SECP256K1_TAG => {
                let (raw_bytes, remainder): ([u8; Self::SECP256K1_LENGTH], _) =
                    FromBytes::from_bytes(remainder)?;
                let public_key = Self::secp256k1_from_bytes(raw_bytes)
                    .map_err(|_error| bytesrepr::Error::Formatting)?;
                Ok((public_key, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        detail::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        detail::deserialize(deserializer)
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for PublicKey {
    fn schema_name() -> String {
        String::from("PublicKey")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some(
            "Hex-encoded cryptographic public key, including the algorithm tag prefix.".to_string(),
        );
        schema_object.metadata().examples = vec![
            json!({
                "name": "SystemPublicKey",
                "description": "A pseudo public key, used for example when the system proposes an \
                immediate switch block after a network upgrade rather than a specific validator. \
                Its hex-encoded value is always '00', as is the corresponding pseudo signature's",
                "value": "00"
            }),
            json!({
                "name": "Ed25519PublicKey",
                "description": "An Ed25519 public key. Its hex-encoded value begins '01' and is \
                followed by 64 characters",
                "value": "018a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"
            }),
            json!({
                "name": "Secp256k1PublicKey",
                "description": "A secp256k1 public key. Its hex-encoded value begins '02' and is \
                followed by 66 characters",
                "value": "0203408e9526316fd1f8def480dd45b2cc72ffd732771c9ceb5d92ffa4051e6ee084"
            }),
        ];
        schema_object.into()
    }
}

impl CLTyped for PublicKey {
    fn cl_type() -> CLType {
        CLType::PublicKey
    }
}

/// A signature of given data.
#[derive(Clone, Copy)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum Signature {
    /// System signature.  Cannot be verified.
    System,
    /// Ed25519 signature.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Ed25519(Ed25519Signature),
    /// Secp256k1 signature.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Secp256k1(Secp256k1Signature),
}

impl Signature {
    /// The length in bytes of a system signature,
    pub const SYSTEM_LENGTH: usize = 0;

    /// The length in bytes of an Ed25519 signature,
    pub const ED25519_LENGTH: usize = ED25519_SIGNATURE_LENGTH;

    /// The length in bytes of a secp256k1 signature
    pub const SECP256K1_LENGTH: usize = SECP256K1_SIGNATURE_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Result<Self, Error> {
        let signature = Ed25519Signature::from_bytes(&bytes);
        Ok(Signature::Ed25519(signature))
    }

    /// Constructs a new secp256k1 variant from a byte array.
    pub fn secp256k1(bytes: [u8; Self::SECP256K1_LENGTH]) -> Result<Self, Error> {
        let signature = Secp256k1Signature::try_from(&bytes[..]).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct secp256k1 signature from {:?}",
                &bytes[..]
            ))
        })?;

        Ok(Signature::Secp256k1(signature))
    }

    fn variant_name(&self) -> &str {
        match self {
            Signature::System => SYSTEM,
            Signature::Ed25519(_) => ED25519,
            Signature::Secp256k1(_) => SECP256K1,
        }
    }
}

impl AsymmetricType<'_> for Signature {
    fn system() -> Self {
        Signature::System
    }

    fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error> {
        let signature = Ed25519Signature::try_from(bytes.as_ref()).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct Ed25519 signature from {:?}",
                bytes.as_ref()
            ))
        })?;
        Ok(Signature::Ed25519(signature))
    }

    fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self, Error> {
        let signature = Secp256k1Signature::try_from(bytes.as_ref()).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct secp256k1 signature from {:?}",
                bytes.as_ref()
            ))
        })?;
        Ok(Signature::Secp256k1(signature))
    }
}

impl Debug for Signature {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Signature::{}({})",
            self.variant_name(),
            base16::encode_lower(&Into::<Vec<u8>>::into(*self))
        )
    }
}

impl Display for Signature {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Sig::{}({:10})",
            self.variant_name(),
            HexFmt(Into::<Vec<u8>>::into(*self))
        )
    }
}

impl PartialOrd for Signature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Signature {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_tag = self.tag();
        let other_tag = other.tag();
        if self_tag == other_tag {
            Into::<Vec<u8>>::into(*self).cmp(&Into::<Vec<u8>>::into(*other))
        } else {
            self_tag.cmp(&other_tag)
        }
    }
}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.tag() == other.tag() && Into::<Vec<u8>>::into(*self) == Into::<Vec<u8>>::into(*other)
    }
}

impl Eq for Signature {}

impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tag().hash(state);
        Into::<Vec<u8>>::into(*self).hash(state);
    }
}

impl Tagged<u8> for Signature {
    fn tag(&self) -> u8 {
        match self {
            Signature::System => SYSTEM_TAG,
            Signature::Ed25519(_) => ED25519_TAG,
            Signature::Secp256k1(_) => SECP256K1_TAG,
        }
    }
}

impl ToBytes for Signature {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                Signature::System => Self::SYSTEM_LENGTH,
                Signature::Ed25519(_) => Self::ED25519_LENGTH,
                Signature::Secp256k1(_) => Self::SECP256K1_LENGTH,
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            Signature::System => {
                writer.push(SYSTEM_TAG);
            }
            Signature::Ed25519(signature) => {
                writer.push(ED25519_TAG);
                writer.extend(signature.to_bytes());
            }
            Signature::Secp256k1(signature) => {
                writer.push(SECP256K1_TAG);
                writer.extend_from_slice(&signature.to_bytes());
            }
        }
        Ok(())
    }
}

impl FromBytes for Signature {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            SYSTEM_TAG => Ok((Signature::System, remainder)),
            ED25519_TAG => {
                let (raw_bytes, remainder): ([u8; Self::ED25519_LENGTH], _) =
                    FromBytes::from_bytes(remainder)?;
                let public_key =
                    Self::ed25519(raw_bytes).map_err(|_error| bytesrepr::Error::Formatting)?;
                Ok((public_key, remainder))
            }
            SECP256K1_TAG => {
                let (raw_bytes, remainder): ([u8; Self::SECP256K1_LENGTH], _) =
                    FromBytes::from_bytes(remainder)?;
                let public_key =
                    Self::secp256k1(raw_bytes).map_err(|_error| bytesrepr::Error::Formatting)?;
                Ok((public_key, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        detail::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        detail::deserialize(deserializer)
    }
}

impl From<&Signature> for Vec<u8> {
    fn from(signature: &Signature) -> Self {
        match signature {
            Signature::System => Vec::new(),
            Signature::Ed25519(signature) => signature.to_bytes().into(),
            Signature::Secp256k1(signature) => (*signature.to_bytes()).into(),
        }
    }
}

impl From<Signature> for Vec<u8> {
    fn from(signature: Signature) -> Self {
        Vec::<u8>::from(&signature)
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for Signature {
    fn schema_name() -> String {
        String::from("Signature")
    }

    fn json_schema(gen: &mut SchemaGenerator) -> Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some(
            "Hex-encoded cryptographic signature, including the algorithm tag prefix.".to_string(),
        );
        schema_object.into()
    }
}

/// Signs the given message using the given key pair.
pub fn sign<T: AsRef<[u8]>>(
    message: T,
    secret_key: &SecretKey,
    public_key: &PublicKey,
) -> Signature {
    match (secret_key, public_key) {
        (SecretKey::System, PublicKey::System) => {
            panic!("cannot create signature with system keys",)
        }
        (SecretKey::Ed25519(secret_key), PublicKey::Ed25519(_public_key)) => {
            let signature = secret_key.sign(message.as_ref());
            Signature::Ed25519(signature)
        }
        (SecretKey::Secp256k1(secret_key), PublicKey::Secp256k1(_public_key)) => {
            let signer = secret_key;
            let signature: Secp256k1Signature = signer
                .try_sign(message.as_ref())
                .expect("should create signature");
            Signature::Secp256k1(signature)
        }
        _ => panic!("secret and public key types must match"),
    }
}

/// Verifies the signature of the given message against the given public key.
pub fn verify<T: AsRef<[u8]>>(
    message: T,
    signature: &Signature,
    public_key: &PublicKey,
) -> Result<(), Error> {
    match (signature, public_key) {
        (Signature::System, _) => Err(Error::AsymmetricKey(String::from(
            "signatures based on the system key cannot be verified",
        ))),
        (Signature::Ed25519(signature), PublicKey::Ed25519(public_key)) => public_key
            .verify_strict(message.as_ref(), signature)
            .map_err(|_| Error::AsymmetricKey(String::from("failed to verify Ed25519 signature"))),
        (Signature::Secp256k1(signature), PublicKey::Secp256k1(public_key)) => {
            let verifier: &Secp256k1PublicKey = public_key;
            verifier
                .verify(message.as_ref(), signature)
                .map_err(|error| {
                    Error::AsymmetricKey(format!("failed to verify secp256k1 signature: {}", error))
                })
        }
        _ => Err(Error::AsymmetricKey(format!(
            "type mismatch between {} and {}",
            signature, public_key
        ))),
    }
}

/// Generates an Ed25519 keypair using the operating system's cryptographically secure random number
/// generator.
#[cfg(any(feature = "std", test))]
pub fn generate_ed25519_keypair() -> (SecretKey, PublicKey) {
    let secret_key = SecretKey::generate_ed25519().unwrap();
    let public_key = PublicKey::from(&secret_key);
    (secret_key, public_key)
}

mod detail {
    use alloc::{string::String, vec::Vec};

    use serde::{de::Error as _deError, Deserialize, Deserializer, Serialize, Serializer};

    use super::{PublicKey, Signature};
    use crate::AsymmetricType;

    /// Used to serialize and deserialize asymmetric key types where the (de)serializer is not a
    /// human-readable type.
    ///
    /// The wrapped contents are the result of calling `t_as_ref()` on the type.
    #[derive(Serialize, Deserialize)]
    pub(super) enum AsymmetricTypeAsBytes {
        System,
        Ed25519(Vec<u8>),
        Secp256k1(Vec<u8>),
    }

    impl From<&PublicKey> for AsymmetricTypeAsBytes {
        fn from(public_key: &PublicKey) -> Self {
            match public_key {
                PublicKey::System => AsymmetricTypeAsBytes::System,
                key @ PublicKey::Ed25519(_) => AsymmetricTypeAsBytes::Ed25519(key.into()),
                key @ PublicKey::Secp256k1(_) => AsymmetricTypeAsBytes::Secp256k1(key.into()),
            }
        }
    }

    impl From<&Signature> for AsymmetricTypeAsBytes {
        fn from(signature: &Signature) -> Self {
            match signature {
                Signature::System => AsymmetricTypeAsBytes::System,
                key @ Signature::Ed25519(_) => AsymmetricTypeAsBytes::Ed25519(key.into()),
                key @ Signature::Secp256k1(_) => AsymmetricTypeAsBytes::Secp256k1(key.into()),
            }
        }
    }

    pub(super) fn serialize<'a, T, S>(value: &'a T, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: AsymmetricType<'a>,
        Vec<u8>: From<&'a T>,
        S: Serializer,
        AsymmetricTypeAsBytes: From<&'a T>,
    {
        if serializer.is_human_readable() {
            return value.to_hex().serialize(serializer);
        }

        AsymmetricTypeAsBytes::from(value).serialize(serializer)
    }

    pub(super) fn deserialize<'a, 'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: AsymmetricType<'a>,
        Vec<u8>: From<&'a T>,
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let hex_string = String::deserialize(deserializer)?;
            let value = T::from_hex(hex_string.as_bytes()).map_err(D::Error::custom)?;
            return Ok(value);
        }

        let as_bytes = AsymmetricTypeAsBytes::deserialize(deserializer)?;
        match as_bytes {
            AsymmetricTypeAsBytes::System => Ok(T::system()),
            AsymmetricTypeAsBytes::Ed25519(raw_bytes) => {
                T::ed25519_from_bytes(raw_bytes).map_err(D::Error::custom)
            }
            AsymmetricTypeAsBytes::Secp256k1(raw_bytes) => {
                T::secp256k1_from_bytes(raw_bytes).map_err(D::Error::custom)
            }
        }
    }
}
