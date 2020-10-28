//! Asymmetric-key types and functions.

use std::{
    cmp::Ordering,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
    iter,
    path::Path,
    result::Result as StdResult,
};

use datasize::DataSize;
use derp::{Der, Tag};
use ed25519_dalek::{self as ed25519, ExpandedSecretKey};
use hex_fmt::HexFmt;
use k256::ecdsa::{
    Signature as Secp256k1Signature, Signer as Secp256k1Signer, Verifier as Secp256k1Verifier,
};
use pem::Pem;
#[cfg(test)]
use rand::{Rng, RngCore};
use serde::{
    de::{Deserializer, Error as SerdeError},
    Deserialize, Serialize, Serializer,
};
use signature::{RandomizedSigner, Signature as Sig, Verifier};
use tracing::info;
use untrusted::Input;

use casper_types::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

use super::{Error, Result};
#[cfg(test)]
use crate::testing::TestRng;
use crate::{
    crypto::hash::hash,
    types::CryptoRngCore,
    utils::{read_file, write_file},
};
use casper_types::account::AccountHash;

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;

const ED25519_TAG: u8 = 1;
const ED25519: &str = "Ed25519";
const ED25519_LOWERCASE: &str = "ed25519";
// See https://tools.ietf.org/html/rfc8410#section-10.3
const ED25519_OBJECT_IDENTIFIER: [u8; 3] = [43, 101, 112];
const ED25519_PEM_SECRET_KEY_TAG: &str = "PRIVATE KEY";
const ED25519_PEM_PUBLIC_KEY_TAG: &str = "PUBLIC KEY";

const SECP256K1_SECRET_KEY_LENGTH: usize = 32;
const SECP256K1_COMPRESSED_PUBLIC_KEY_LENGTH: usize = 33;
const SECP256K1_SIGNATURE_LENGTH: usize = 64;
const SECP256K1_TAG: u8 = 2;
const SECP256K1: &str = "Secp256k1";
const SECP256K1_LOWERCASE: &str = "secp256k1";
// See https://www.secg.org/sec1-v2.pdf#subsection.C.4
const EC_PUBLIC_KEY_OBJECT_IDENTIFIER: [u8; 7] = [42, 134, 72, 206, 61, 2, 1];
const SECP256K1_OBJECT_IDENTIFIER: [u8; 5] = [43, 129, 4, 0, 10];
const SECP256K1_PEM_SECRET_KEY_TAG: &str = "EC PRIVATE KEY";
const SECP256K1_PEM_PUBLIC_KEY_TAG: &str = "PUBLIC KEY";

/// A secret or private asymmetric key.
#[derive(DataSize)]
pub enum SecretKey {
    /// Ed25519 secret key.
    #[data_size(skip)] // Manually verified to have no data on the heap.
    Ed25519(ed25519::SecretKey),
    /// secp256k1 secret key.
    #[data_size(skip)] // Manually verified to have no data on the heap.
    Secp256k1(k256::SecretKey),
}

impl Clone for SecretKey {
    fn clone(&self) -> Self {
        match self {
            SecretKey::Ed25519(sk) => Self::ed25519_from_bytes(sk.as_ref()).unwrap(),
            SecretKey::Secp256k1(sk) => Self::secp256k1_from_bytes(sk.as_bytes()).unwrap(),
        }
    }
}

impl SecretKey {
    /// The length in bytes of an Ed25519 secret key.
    pub const ED25519_LENGTH: usize = ed25519::SECRET_KEY_LENGTH;

    /// The length in bytes of a secp256k1 secret key.
    pub const SECP256K1_LENGTH: usize = SECP256K1_SECRET_KEY_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn new_ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Self {
        // safe to unwrap as `SecretKey::from_bytes` can only fail if the provided slice is the
        // wrong length.
        SecretKey::Ed25519(ed25519::SecretKey::from_bytes(&bytes).unwrap())
    }

    /// Constructs a new secp256k1 variant from a byte array.
    pub fn new_secp256k1(bytes: [u8; Self::SECP256K1_LENGTH]) -> Self {
        // safe to unwrap as `SecretKey::from_bytes` can only fail if the provided slice is the
        // wrong length.
        SecretKey::Secp256k1(k256::SecretKey::from_bytes(&bytes).unwrap())
    }

    /// Constructs a new Ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Ok(SecretKey::Ed25519(
            ed25519::SecretKey::from_bytes(bytes.as_ref()).map_err(|_| {
                Error::AsymmetricKey(format!(
                    "failed to construct Ed25519 secret key.  Expected {} bytes, got {} bytes.",
                    Self::ED25519_LENGTH,
                    bytes.as_ref().len()
                ))
            })?,
        ))
    }

    /// Constructs a new secp256k1 variant from a byte slice.
    pub fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Ok(SecretKey::Secp256k1(
            k256::SecretKey::from_bytes(bytes.as_ref()).map_err(|_| {
                Error::AsymmetricKey(format!(
                    "failed to construct secp256k1 secret key.  Expected {} bytes, got {} bytes.",
                    Self::SECP256K1_LENGTH,
                    bytes.as_ref().len()
                ))
            })?,
        ))
    }

    /// Constructs a new Ed25519 variant using the operating system's cryptographically secure
    /// random number generator.
    pub fn generate_ed25519() -> Self {
        let mut bytes = [0u8; Self::ED25519_LENGTH];
        getrandom::getrandom(&mut bytes[..]).expect("RNG failure!");
        SecretKey::new_ed25519(bytes)
    }

    /// Constructs a new secp256k1 variant using the operating system's cryptographically secure
    /// random number generator.
    pub fn generate_secp256k1() -> Self {
        let mut bytes = [0u8; Self::SECP256K1_LENGTH];
        getrandom::getrandom(&mut bytes[..]).expect("RNG failure!");
        SecretKey::new_secp256k1(bytes)
    }

    /// Exposes the secret values of the key as a byte slice.
    pub fn as_secret_slice(&self) -> &[u8] {
        match self {
            SecretKey::Ed25519(secret_key) => secret_key.as_ref(),
            SecretKey::Secp256k1(secret_key) => secret_key.as_bytes().as_slice(),
        }
    }

    /// Attempts to write the secret key bytes to the configured file path.
    pub fn to_file<P: AsRef<Path>>(&self, file: P) -> Result<()> {
        write_file(file, self.to_pem()?).map_err(Error::SecretKeySave)
    }

    /// Attempts to read the secret key bytes from configured file path.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self> {
        let data = read_file(file).map_err(Error::SecretKeyLoad)?;
        Self::from_pem(data)
    }

    /// Duplicates a secret key.
    ///
    /// Only available for testing and named other than `clone` to prevent accidental use.
    #[cfg(test)]
    pub fn duplicate(&self) -> Self {
        match self {
            SecretKey::Ed25519(secret_key) => {
                Self::ed25519_from_bytes(secret_key.as_ref()).expect("could not copy secret key")
            }
            SecretKey::Secp256k1(secret_key) => {
                Self::secp256k1_from_bytes(secret_key.as_bytes().as_slice())
                    .expect("could not copy secret key")
            }
        }
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        if rng.gen() {
            Self::random_ed25519(rng)
        } else {
            Self::random_secp256k1(rng)
        }
    }

    /// Generates a random Ed25519 instance using a `TestRng`.
    #[cfg(test)]
    pub fn random_ed25519(rng: &mut TestRng) -> Self {
        let mut bytes = [0u8; Self::ED25519_LENGTH];
        rng.fill_bytes(&mut bytes[..]);
        SecretKey::new_ed25519(bytes)
    }

    /// Generates a random secp256k1 instance using a `TestRng`.
    #[cfg(test)]
    pub fn random_secp256k1(rng: &mut TestRng) -> Self {
        let mut bytes = [0u8; Self::SECP256K1_LENGTH];
        rng.fill_bytes(&mut bytes[..]);
        SecretKey::new_secp256k1(bytes)
    }

    fn tag(&self) -> u8 {
        match self {
            SecretKey::Ed25519(_) => ED25519_TAG,
            SecretKey::Secp256k1(_) => SECP256K1_TAG,
        }
    }

    /// DER encodes the secret key.
    fn to_der(&self) -> Result<Vec<u8>> {
        match self {
            SecretKey::Ed25519(secret_key) => {
                // See https://tools.ietf.org/html/rfc8410#section-10.3
                let mut key_bytes = vec![];
                let mut der = Der::new(&mut key_bytes);
                der.octet_string(secret_key.as_ref())?;

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
                    der.octet_string(secret_key.as_bytes().as_slice())?;
                    der.element(Tag::ContextSpecificConstructed0, &oid_bytes)
                })?;
                Ok(encoded)
            }
        }
    }

    /// Decodes a secret key from a DER-encoded slice.
    fn from_der<T: AsRef<[u8]>>(input: T) -> Result<Self> {
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
            ED25519_TAG => SecretKey::ed25519_from_bytes(raw_bytes),
            SECP256K1_TAG => SecretKey::secp256k1_from_bytes(raw_bytes),
            _ => unreachable!(),
        }
    }

    /// PEM encodes the secret key.
    fn to_pem(&self) -> Result<String> {
        let tag = match self {
            SecretKey::Ed25519(_) => ED25519_PEM_SECRET_KEY_TAG.to_string(),
            SecretKey::Secp256k1(_) => SECP256K1_PEM_SECRET_KEY_TAG.to_string(),
        };
        let contents = self.to_der()?;
        let pem = Pem { tag, contents };
        Ok(pem::encode(&pem))
    }

    /// Decodes a secret key from a PEM-encoded slice.
    fn from_pem<T: AsRef<[u8]>>(input: T) -> Result<Self> {
        let pem = pem::parse(input)?;

        let secret_key = Self::from_der(&pem.contents)?;

        let bad_tag = |expected_tag: &str| {
            Error::FromPem(format!(
                "invalid tag: expected {}, got {}",
                expected_tag, pem.tag
            ))
        };

        match secret_key {
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
}

impl Debug for SecretKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            SecretKey::Ed25519(_) => write!(formatter, "SecretKey::{}(...)", ED25519),
            SecretKey::Secp256k1(_) => write!(formatter, "SecretKey::{}(...)", SECP256K1),
        }
    }
}

impl Display for SecretKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, formatter)
    }
}

impl Serialize for SecretKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> StdResult<S::Ok, S::Error> {
        serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for SecretKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> StdResult<Self, D::Error> {
        deserialize(deserializer)
    }
}

/// A public asymmetric key.
#[derive(Clone, Copy, DataSize, Eq, PartialEq)]
pub enum PublicKey {
    /// Ed25519 public key.
    #[data_size(skip)] // Manually verified to have no data on the heap.
    Ed25519(ed25519::PublicKey),
    /// secp256k1 public key.
    #[data_size(skip)] // Manually verified to have no data on the heap.
    Secp256k1(k256::PublicKey),
}

impl PublicKey {
    /// The length in bytes of an Ed25519 public key.
    pub const ED25519_LENGTH: usize = ed25519::PUBLIC_KEY_LENGTH;

    /// The length in bytes of a secp256k1 public key.
    pub const SECP256K1_LENGTH: usize = SECP256K1_COMPRESSED_PUBLIC_KEY_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn new_ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Result<Self> {
        Ok(PublicKey::Ed25519(
            ed25519::PublicKey::from_bytes(&bytes).map_err(|_| {
                Error::AsymmetricKey(format!(
                    "failed to construct Ed25519 public key from {:?}",
                    bytes
                ))
            })?,
        ))
    }

    /// Constructs a new secp256k1 variant from a byte array.
    pub fn new_secp256k1(bytes: [u8; Self::SECP256K1_LENGTH]) -> Result<Self> {
        Ok(PublicKey::Secp256k1(
            k256::PublicKey::from_bytes(&bytes[..]).ok_or_else(|| {
                Error::AsymmetricKey(format!(
                    "failed to construct secp256k1 public key from {:?}",
                    &bytes[..]
                ))
            })?,
        ))
    }

    /// Constructs a new Ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Ok(PublicKey::Ed25519(
            ed25519::PublicKey::from_bytes(bytes.as_ref()).map_err(|_| {
                Error::AsymmetricKey(format!(
                    "failed to construct Ed25519 public key.  Expected {} bytes, got {} bytes.",
                    Self::ED25519_LENGTH,
                    bytes.as_ref().len()
                ))
            })?,
        ))
    }

    /// Constructs a new secp256k1 variant from a byte slice.
    pub fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        let mut public_key = k256::PublicKey::from_bytes(bytes.as_ref()).ok_or_else(|| {
            Error::AsymmetricKey(format!(
                "failed to construct secp256k1 public key.  Expected {} bytes, got {} bytes.",
                Self::SECP256K1_LENGTH,
                bytes.as_ref().len()
            ))
        })?;
        public_key.compress();
        Ok(PublicKey::Secp256k1(public_key))
    }

    /// Creates an `AccountHash` from a given `PublicKey` instance.
    pub fn to_account_hash(&self) -> AccountHash {
        // As explained here:
        // https://casperlabs.atlassian.net/wiki/spaces/EN/pages/446431524/Design+for+supporting+multiple+signature+algorithms.
        let algorithm_name = match self {
            PublicKey::Ed25519(_) => ED25519_LOWERCASE,
            PublicKey::Secp256k1(_) => SECP256K1_LOWERCASE,
        };
        let public_key_bytes = self.as_ref();

        // Prepare preimage based on the public key parameters.
        let preimage = {
            let mut data = Vec::with_capacity(algorithm_name.len() + public_key_bytes.len() + 1);
            data.extend(algorithm_name.as_bytes());
            data.push(0);
            data.extend(public_key_bytes);
            data
        };
        // Hash the preimage data using blake2b256 and return it.
        let digest = hash(&preimage);
        AccountHash::new(digest.to_array())
    }

    /// Attempts to write the public key PEM-encoded to the configured file path.
    pub fn to_file<P: AsRef<Path>>(&self, file: P) -> Result<()> {
        write_file(file, self.to_pem()?).map_err(Error::PublicKeySave)
    }

    /// Attempts to read the public key bytes from configured PEM-encoded file.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self> {
        let data = read_file(file).map_err(Error::PublicKeyLoad)?;
        Self::from_pem(data)
    }

    /// Converts the public key to hex, where the first byte represents the algorithm tag.
    pub fn to_hex(&self) -> String {
        to_hex(self)
    }

    /// Tries to decode a public key from its hex-representation.  The hex format should be as
    /// produced by `PublicKey::to_hex()`.
    pub fn from_hex<T: AsRef<[u8]>>(input: T) -> Result<Self> {
        from_hex(input)
    }

    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random(rng);
        PublicKey::from(&secret_key)
    }

    /// Generates a random Ed25519 instance using a `TestRng`.
    #[cfg(test)]
    pub fn random_ed25519(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random_ed25519(rng);
        PublicKey::from(&secret_key)
    }

    /// Generates a random secp256k1 instance using a `TestRng`.
    #[cfg(test)]
    pub fn random_secp256k1(rng: &mut TestRng) -> Self {
        let secret_key = SecretKey::random_secp256k1(rng);
        PublicKey::from(&secret_key)
    }

    fn tag(&self) -> u8 {
        match self {
            PublicKey::Ed25519(_) => ED25519_TAG,
            PublicKey::Secp256k1(_) => SECP256K1_TAG,
        }
    }

    fn variant_name(&self) -> &str {
        match self {
            PublicKey::Ed25519(_) => ED25519,
            PublicKey::Secp256k1(_) => SECP256K1,
        }
    }

    /// DER encodes the public key.
    fn to_der(&self) -> Result<Vec<u8>> {
        match self {
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
                    der.bit_string(0, public_key.as_ref())
                })?;
                Ok(encoded)
            }
        }
    }

    /// Decodes a public key from a DER-encoded slice.
    fn from_der<T: AsRef<[u8]>>(input: T) -> Result<Self> {
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
            ED25519_TAG => PublicKey::ed25519_from_bytes(raw_bytes),
            SECP256K1_TAG => PublicKey::secp256k1_from_bytes(raw_bytes),
            _ => unreachable!(),
        }
    }

    /// PEM encodes the public key.
    fn to_pem(&self) -> Result<String> {
        let tag = match self {
            PublicKey::Ed25519(_) => ED25519_PEM_PUBLIC_KEY_TAG.to_string(),
            PublicKey::Secp256k1(_) => SECP256K1_PEM_PUBLIC_KEY_TAG.to_string(),
        };
        let contents = self.to_der()?;
        let pem = Pem { tag, contents };
        Ok(pem::encode(&pem))
    }

    /// Decodes a public key from a PEM-encoded slice.
    fn from_pem<T: AsRef<[u8]>>(input: T) -> Result<Self> {
        let pem = pem::parse(input)?;

        let public_key = Self::from_der(&pem.contents)?;

        let bad_tag = |expected_tag: &str| {
            Error::FromPem(format!(
                "invalid tag: expected {}, got {}",
                expected_tag, pem.tag
            ))
        };

        match public_key {
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
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        match self {
            PublicKey::Ed25519(public_key) => public_key.as_ref(),
            PublicKey::Secp256k1(public_key) => public_key.as_ref(),
        }
    }
}

impl Ord for PublicKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_tag = self.tag();
        let other_tag = other.tag();
        if self_tag == other_tag {
            self.as_ref().cmp(other.as_ref())
        } else {
            self_tag.cmp(&other_tag)
        }
    }
}

impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// This implementation of `Hash` agrees with the derived `PartialEq`.  It's required since
// `ed25519_dalek::PublicKey` doesn't implement `Hash`.
#[allow(clippy::derive_hash_xor_eq)]
impl Hash for PublicKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tag().hash(state);
        self.as_ref().hash(state);
    }
}

impl Debug for PublicKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PublicKey::{}({})",
            self.variant_name(),
            HexFmt(self)
        )
    }
}

impl Display for PublicKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "PubKey::{}({:10})",
            self.variant_name(),
            HexFmt(self)
        )
    }
}

impl From<&SecretKey> for PublicKey {
    fn from(secret_key: &SecretKey) -> PublicKey {
        match secret_key {
            SecretKey::Ed25519(secret_key) => PublicKey::Ed25519(secret_key.into()),
            SecretKey::Secp256k1(secret_key) => PublicKey::Secp256k1(
                k256::PublicKey::from_secret_key(secret_key, true)
                    .expect("should create secp256k1 public key"),
            ),
        }
    }
}

impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> StdResult<S::Ok, S::Error> {
        serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> StdResult<Self, D::Error> {
        deserialize(deserializer)
    }
}

impl ToBytes for PublicKey {
    fn to_bytes(&self) -> StdResult<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            PublicKey::Ed25519(public_key) => {
                buffer.insert(0, ED25519_TAG);
                buffer.extend(public_key.as_ref().to_vec().into_bytes()?);
            }
            PublicKey::Secp256k1(public_key) => {
                buffer.insert(0, SECP256K1_TAG);
                buffer.extend(public_key.as_ref().to_vec().into_bytes()?);
            }
        }
        Ok(buffer)
    }

    // TODO: implement ToBytes for `&[u8]` to avoid allocating via `to_vec()` here.
    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                PublicKey::Ed25519(public_key) => public_key.as_ref().to_vec().serialized_length(),
                PublicKey::Secp256k1(public_key) => {
                    public_key.as_ref().to_vec().serialized_length()
                }
            }
    }
}

impl FromBytes for PublicKey {
    fn from_bytes(bytes: &[u8]) -> StdResult<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ED25519_TAG => {
                let (raw_bytes, remainder) = Vec::<u8>::from_bytes(remainder)?;
                let public_key = Self::ed25519_from_bytes(&raw_bytes).map_err(|error| {
                    info!("failed deserializing to public key: {}", error);
                    bytesrepr::Error::Formatting
                })?;
                Ok((public_key, remainder))
            }
            SECP256K1_TAG => {
                let (raw_bytes, remainder) = Vec::<u8>::from_bytes(remainder)?;
                let public_key = Self::secp256k1_from_bytes(&raw_bytes).map_err(|error| {
                    info!("failed deserializing to public key: {}", error);
                    bytesrepr::Error::Formatting
                })?;
                Ok((public_key, remainder))
            }
            _ => {
                info!("failed deserializing to public key: invalid tag {}", tag);
                Err(bytesrepr::Error::Formatting)
            }
        }
    }
}

/// Generates an Ed25519 keypair using the operating system's cryptographically secure random number
/// generator.
pub fn generate_ed25519_keypair() -> (SecretKey, PublicKey) {
    let secret_key = SecretKey::generate_ed25519();
    let public_key = PublicKey::from(&secret_key);
    (secret_key, public_key)
}

impl TryFrom<casper_types::PublicKey> for PublicKey {
    type Error = Error;
    fn try_from(value: casper_types::PublicKey) -> Result<Self> {
        match value {
            casper_types::PublicKey::Ed25519(bytes) => PublicKey::new_ed25519(bytes),
            casper_types::PublicKey::Secp256k1(bytes) => PublicKey::new_secp256k1(bytes.value()),
        }
    }
}

impl From<PublicKey> for casper_types::PublicKey {
    fn from(value: PublicKey) -> Self {
        match value {
            PublicKey::Ed25519(ed25519) => casper_types::PublicKey::Ed25519(ed25519.to_bytes()),
            PublicKey::Secp256k1(mut secp256k1) => {
                secp256k1.compress();
                let mut bytes = [0; PublicKey::SECP256K1_LENGTH];
                bytes.copy_from_slice(secp256k1.as_bytes());
                casper_types::PublicKey::Secp256k1(bytes.into())
            }
        }
    }
}

/// Generates a secp256k1 keypair using the operating system's cryptographically secure random
/// number generator.
pub fn generate_secp256k1_keypair() -> (SecretKey, PublicKey) {
    let secret_key = SecretKey::generate_secp256k1();
    let public_key = PublicKey::from(&secret_key);
    (secret_key, public_key)
}

/// A signature of given data.
#[derive(Clone, Copy, DataSize)]
pub enum Signature {
    /// Ed25519 signature.
    //
    // This is held as a byte array rather than an `ed25519_dalek::Signature` as that type doesn't
    // implement `AsRef` amongst other common traits.  In order to implement these common traits,
    // it is convenient and cheap to use `signature.as_ref()`.
    Ed25519([u8; ed25519::SIGNATURE_LENGTH]),
    /// secp256k1 signature.
    #[data_size(skip)] // Manually verified to have no data on the heap.
    Secp256k1(Secp256k1Signature),
}

impl Signature {
    /// The length in bytes of an Ed25519 signature,
    pub const ED25519_LENGTH: usize = ed25519::SIGNATURE_LENGTH;

    /// The length in bytes of a secp256k1 signature
    pub const SECP256K1_LENGTH: usize = SECP256K1_SIGNATURE_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn new_ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Result<Self> {
        let signature = ed25519::Signature::from_bytes(&bytes).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct Ed25519 signature from {:?}",
                &bytes[..]
            ))
        })?;

        Ok(Signature::Ed25519(signature.to_bytes()))
    }

    /// Constructs a new secp256k1 variant from a byte array.
    pub fn new_secp256k1(bytes: [u8; Self::SECP256K1_LENGTH]) -> Result<Self> {
        let signature = Secp256k1Signature::try_from(&bytes[..]).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct secp256k1 signature from {:?}",
                &bytes[..]
            ))
        })?;

        Ok(Signature::Secp256k1(signature))
    }

    /// Constructs a new Ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        let signature = ed25519::Signature::from_bytes(bytes.as_ref()).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct Ed25519 signature from {:?}",
                bytes.as_ref()
            ))
        })?;

        Ok(Signature::Ed25519(signature.to_bytes()))
    }

    /// Constructs a new secp256k1 variant from a byte slice.
    pub fn secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        let signature = Secp256k1Signature::try_from(bytes.as_ref()).map_err(|_| {
            Error::AsymmetricKey(format!(
                "failed to construct secp256k1 signature from {:?}",
                bytes.as_ref()
            ))
        })?;

        Ok(Signature::Secp256k1(signature))
    }

    /// Converts the signature to hex, where the first byte represents the algorithm tag.
    pub fn to_hex(&self) -> String {
        to_hex(self)
    }

    /// Tries to decode a signature from its hex-representation.  The hex format should be as
    /// produced by `Signature::to_hex()`.
    pub fn from_hex<T: AsRef<[u8]>>(input: T) -> Result<Self> {
        from_hex(input)
    }

    fn tag(&self) -> u8 {
        match self {
            Signature::Ed25519(_) => ED25519_TAG,
            Signature::Secp256k1(_) => SECP256K1_TAG,
        }
    }

    fn variant_name(&self) -> &str {
        match self {
            Signature::Ed25519(_) => ED25519,
            Signature::Secp256k1(_) => SECP256K1,
        }
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        match self {
            Signature::Ed25519(signature) => signature.as_ref(),
            Signature::Secp256k1(signature) => signature.as_ref(),
        }
    }
}

impl Ord for Signature {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_tag = self.tag();
        let other_tag = other.tag();
        if self_tag == other_tag {
            self.as_ref().cmp(other.as_ref())
        } else {
            self_tag.cmp(&other_tag)
        }
    }
}

impl PartialOrd for Signature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Signature {}

impl PartialEq for Signature {
    fn eq(&self, other: &Self) -> bool {
        self.tag() == other.tag() && self.as_ref() == other.as_ref()
    }
}

impl Hash for Signature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tag().hash(state);
        self.as_ref().hash(state);
    }
}

impl Debug for Signature {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Signature::{}({})",
            self.variant_name(),
            HexFmt(self)
        )
    }
}

impl Display for Signature {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Sig::{}({:10})",
            self.variant_name(),
            HexFmt(self)
        )
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> StdResult<S::Ok, S::Error> {
        serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> StdResult<Self, D::Error> {
        deserialize(deserializer)
    }
}

trait AsymmetricType {
    fn t_as_ref(&self) -> &[u8];
    fn t_tag(&self) -> u8;
    fn t_ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self>
    where
        Self: Sized;
    fn t_secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self>
    where
        Self: Sized;
}

impl AsymmetricType for SecretKey {
    fn t_as_ref(&self) -> &[u8] {
        self.as_secret_slice()
    }

    fn t_tag(&self) -> u8 {
        self.tag()
    }

    fn t_ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Self::ed25519_from_bytes(bytes)
    }

    fn t_secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Self::secp256k1_from_bytes(bytes)
    }
}

impl AsymmetricType for PublicKey {
    fn t_as_ref(&self) -> &[u8] {
        self.as_ref()
    }

    fn t_tag(&self) -> u8 {
        self.tag()
    }

    fn t_ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Self::ed25519_from_bytes(bytes)
    }

    fn t_secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Self::secp256k1_from_bytes(bytes)
    }
}

impl AsymmetricType for Signature {
    fn t_as_ref(&self) -> &[u8] {
        self.as_ref()
    }

    fn t_tag(&self) -> u8 {
        self.tag()
    }

    fn t_ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Self::ed25519_from_bytes(bytes)
    }

    fn t_secp256k1_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Self::secp256k1_from_bytes(bytes)
    }
}

/// Converts `A` to hex, where the first byte represents the algorithm tag.
fn to_hex<A: AsymmetricType>(value: &A) -> String {
    let bytes = iter::once(&value.t_tag())
        .chain(value.t_as_ref())
        .copied()
        .collect::<Vec<u8>>();
    hex::encode(bytes)
}

/// Tries to decode `A` from its hex-representation.  The hex format should be as produced by
/// `A::to_hex()`.
fn from_hex<A: AsymmetricType, T: AsRef<[u8]>>(input: T) -> Result<A> {
    if input.as_ref().len() < 2 {
        return Err(Error::AsymmetricKey("too short".to_string()));
    }

    let (tag_bytes, key_bytes) = input.as_ref().split_at(2);
    let mut tag = [0u8; 1];
    hex::decode_to_slice(tag_bytes, tag.as_mut())?;

    match tag[0] {
        ED25519_TAG => {
            let bytes = hex::decode(key_bytes)?;
            A::t_ed25519_from_bytes(&bytes)
        }
        SECP256K1_TAG => {
            let bytes = hex::decode(key_bytes)?;
            A::t_secp256k1_from_bytes(&bytes)
        }
        _ => Err(Error::AsymmetricKey(format!(
            "invalid tag.  Expected {} or {}, got {}",
            ED25519_TAG, SECP256K1_TAG, tag[0]
        ))),
    }
}

/// Used to serialize and deserialize asymmetric key types where the (de)serializer is not a
/// human-readable type.
///
/// The wrapped contents are the result of calling `t_as_ref()` on the type.
#[derive(Serialize, Deserialize)]
enum AsymmetricTypeAsBytes<'a> {
    Ed25519(&'a [u8]),
    Secp256k1(&'a [u8]),
}

impl<'a> From<&'a SecretKey> for AsymmetricTypeAsBytes<'a> {
    fn from(secret_key: &'a SecretKey) -> Self {
        match secret_key {
            SecretKey::Ed25519(ed25519) => AsymmetricTypeAsBytes::Ed25519(ed25519.as_ref()),
            SecretKey::Secp256k1(secp256k1) => {
                AsymmetricTypeAsBytes::Secp256k1(secp256k1.as_bytes().as_slice())
            }
        }
    }
}

impl<'a> From<&'a PublicKey> for AsymmetricTypeAsBytes<'a> {
    fn from(public_key: &'a PublicKey) -> Self {
        match public_key {
            PublicKey::Ed25519(ed25519) => AsymmetricTypeAsBytes::Ed25519(ed25519.as_ref()),
            PublicKey::Secp256k1(secp256k1) => AsymmetricTypeAsBytes::Secp256k1(secp256k1.as_ref()),
        }
    }
}

impl<'a> From<&'a Signature> for AsymmetricTypeAsBytes<'a> {
    fn from(signature: &'a Signature) -> Self {
        match signature {
            Signature::Ed25519(ed25519) => AsymmetricTypeAsBytes::Ed25519(ed25519.as_ref()),
            Signature::Secp256k1(secp256k1) => AsymmetricTypeAsBytes::Secp256k1(secp256k1.as_ref()),
        }
    }
}

fn serialize<'a, T, S>(value: &'a T, serializer: S) -> StdResult<S::Ok, S::Error>
where
    T: AsymmetricType,
    AsymmetricTypeAsBytes<'a>: From<&'a T>,
    S: Serializer,
{
    if serializer.is_human_readable() {
        return to_hex(value).serialize(serializer);
    }

    AsymmetricTypeAsBytes::from(value).serialize(serializer)
}

fn deserialize<'de, T: AsymmetricType, D: Deserializer<'de>>(
    deserializer: D,
) -> StdResult<T, D::Error> {
    if deserializer.is_human_readable() {
        let hex_string = String::deserialize(deserializer)?;
        let value = from_hex(hex_string.as_bytes()).map_err(D::Error::custom)?;
        return Ok(value);
    }

    let as_bytes = AsymmetricTypeAsBytes::deserialize(deserializer)?;
    match as_bytes {
        AsymmetricTypeAsBytes::Ed25519(raw_bytes) => {
            T::t_ed25519_from_bytes(raw_bytes).map_err(D::Error::custom)
        }
        AsymmetricTypeAsBytes::Secp256k1(raw_bytes) => {
            T::t_secp256k1_from_bytes(raw_bytes).map_err(D::Error::custom)
        }
    }
}

/// Signs the given message using the given key pair.
pub fn sign<T: AsRef<[u8]>>(
    message: T,
    secret_key: &SecretKey,
    public_key: &PublicKey,
    rng: &mut dyn CryptoRngCore,
) -> Signature {
    match (secret_key, public_key) {
        (SecretKey::Ed25519(secret_key), PublicKey::Ed25519(public_key)) => {
            let expanded_secret_key = ExpandedSecretKey::from(secret_key);
            let signature = expanded_secret_key.sign(message.as_ref(), public_key);
            Signature::Ed25519(signature.to_bytes())
        }
        (SecretKey::Secp256k1(secret_key), PublicKey::Secp256k1(_public_key)) => {
            let signer = Secp256k1Signer::new(secret_key).expect("should create secp256k1 signer");
            Signature::Secp256k1(signer.sign_with_rng(rng, message.as_ref()))
        }
        _ => panic!("secret and public key types must match"),
    }
}

/// Verifies the signature of the given message against the given public key.
pub fn verify<T: AsRef<[u8]>>(
    message: T,
    signature: &Signature,
    public_key: &PublicKey,
) -> Result<()> {
    match (signature, public_key) {
        (Signature::Ed25519(signature), PublicKey::Ed25519(public_key)) => public_key
            .verify_strict(
                message.as_ref(),
                &ed25519::Signature::from_bytes(signature).map_err(|_| {
                    Error::AsymmetricKey(format!(
                        "failed to construct Ed25519 signature from {:?}",
                        &signature[..]
                    ))
                })?,
            )
            .map_err(|_| Error::AsymmetricKey(String::from("failed to verify Ed25519 signature"))),
        (Signature::Secp256k1(signature), PublicKey::Secp256k1(pub_key)) => {
            let verifier = Secp256k1Verifier::new(pub_key).map_err(|error| {
                Error::AsymmetricKey(format!(
                    "failed to create secp256k1 verifier from {}: {}",
                    public_key, error
                ))
            })?;

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

#[cfg(test)]
mod tests {
    use std::{
        cmp::Ordering,
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
        iter,
    };

    use openssl::pkey::{PKey, Private, Public};

    use super::*;

    type OpenSSLSecretKey = PKey<Private>;
    type OpenSSLPublicKey = PKey<Public>;

    fn secret_key_serialization_roundtrip(secret_key: SecretKey) {
        // Try to/from bincode.
        let serialized = bincode::serialize(&secret_key).unwrap();
        let deserialized: SecretKey = bincode::deserialize(&serialized).unwrap();
        assert_eq!(secret_key.as_secret_slice(), deserialized.as_secret_slice());
        assert_eq!(secret_key.tag(), deserialized.tag());

        // Try to/from JSON.
        let serialized = serde_json::to_vec_pretty(&secret_key).unwrap();
        let deserialized: SecretKey = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(secret_key.as_secret_slice(), deserialized.as_secret_slice());
        assert_eq!(secret_key.tag(), deserialized.tag());
    }

    fn secret_key_der_roundtrip(secret_key: SecretKey) {
        let der_encoded = secret_key.to_der().unwrap();
        let decoded = SecretKey::from_der(&der_encoded).unwrap();
        assert_eq!(secret_key.as_secret_slice(), decoded.as_secret_slice());
        assert_eq!(secret_key.tag(), decoded.tag());

        // Ensure malformed encoded version fails to decode.
        SecretKey::from_der(&der_encoded[1..]).unwrap_err();
    }

    fn secret_key_pem_roundtrip(secret_key: SecretKey) {
        let pem_encoded = secret_key.to_pem().unwrap();
        let decoded = SecretKey::from_pem(pem_encoded.as_bytes()).unwrap();
        assert_eq!(secret_key.as_secret_slice(), decoded.as_secret_slice());
        assert_eq!(secret_key.tag(), decoded.tag());

        // Check PEM-encoded can be decoded by openssl.
        let _ = OpenSSLSecretKey::private_key_from_pem(pem_encoded.as_bytes()).unwrap();

        // Ensure malformed encoded version fails to decode.
        SecretKey::from_pem(&pem_encoded[1..]).unwrap_err();
    }

    fn known_secret_key_to_pem(known_key_hex: &str, known_key_pem: &str, expected_tag: u8) {
        let key_bytes = hex::decode(known_key_hex).unwrap();
        let decoded = SecretKey::from_pem(known_key_pem.as_bytes()).unwrap();
        assert_eq!(key_bytes, decoded.as_secret_slice());
        assert_eq!(expected_tag, decoded.tag());
    }

    fn secret_key_file_roundtrip(secret_key: SecretKey) {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test_secret_key.pem");

        secret_key.to_file(&path).unwrap();
        let decoded = SecretKey::from_file(&path).unwrap();
        assert_eq!(secret_key.as_secret_slice(), decoded.as_secret_slice());
        assert_eq!(secret_key.tag(), decoded.tag());
    }

    fn public_key_serialization_roundtrip(public_key: PublicKey) {
        // Try to/from bincode.
        let serialized = bincode::serialize(&public_key).unwrap();
        let deserialized = bincode::deserialize(&serialized).unwrap();
        assert_eq!(public_key, deserialized);
        assert_eq!(public_key.tag(), deserialized.tag());

        // Try to/from JSON.
        let serialized = serde_json::to_vec_pretty(&public_key).unwrap();
        let deserialized = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(public_key, deserialized);
        assert_eq!(public_key.tag(), deserialized.tag());

        // Using bytesrepr.
        bytesrepr::test_serialization_roundtrip(&public_key);
    }

    fn public_key_der_roundtrip(public_key: PublicKey) {
        let der_encoded = public_key.to_der().unwrap();
        let decoded = PublicKey::from_der(&der_encoded).unwrap();
        assert_eq!(public_key, decoded);

        // Check DER-encoded can be decoded by openssl.
        let _ = OpenSSLPublicKey::public_key_from_der(&der_encoded).unwrap();

        // Ensure malformed encoded version fails to decode.
        PublicKey::from_der(&der_encoded[1..]).unwrap_err();
    }

    fn public_key_pem_roundtrip(public_key: PublicKey) {
        let pem_encoded = public_key.to_pem().unwrap();
        let decoded = PublicKey::from_pem(pem_encoded.as_bytes()).unwrap();
        assert_eq!(public_key, decoded);
        assert_eq!(public_key.tag(), decoded.tag());

        // Check PEM-encoded can be decoded by openssl.
        let _ = OpenSSLPublicKey::public_key_from_pem(pem_encoded.as_bytes()).unwrap();

        // Ensure malformed encoded version fails to decode.
        PublicKey::from_pem(&pem_encoded[1..]).unwrap_err();
    }

    fn known_public_key_to_pem(known_key_hex: &str, known_key_pem: &str) {
        let key_bytes = hex::decode(known_key_hex).unwrap();
        let decoded = PublicKey::from_pem(known_key_pem.as_bytes()).unwrap();
        assert_eq!(key_bytes, decoded.as_ref());
    }

    fn public_key_file_roundtrip(public_key: PublicKey) {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("test_public_key.pem");

        public_key.to_file(&path).unwrap();
        let decoded = PublicKey::from_file(&path).unwrap();
        assert_eq!(public_key, decoded);
    }

    fn public_key_hex_roundtrip(public_key: PublicKey) {
        let hex_encoded = public_key.to_hex();
        let decoded = PublicKey::from_hex(hex_encoded.as_bytes()).unwrap();
        assert_eq!(public_key, decoded);
        assert_eq!(public_key.tag(), decoded.tag());

        // Ensure malformed encoded version fails to decode.
        PublicKey::from_hex(&hex_encoded[..1]).unwrap_err();
        PublicKey::from_hex(&hex_encoded[1..]).unwrap_err();
    }

    fn signature_serialization_roundtrip(signature: Signature) {
        // Try to/from bincode.
        let serialized = bincode::serialize(&signature).unwrap();
        let deserialized: Signature = bincode::deserialize(&serialized).unwrap();
        assert_eq!(signature, deserialized);
        assert_eq!(signature.tag(), deserialized.tag());

        // Try to/from JSON.
        let serialized = serde_json::to_vec_pretty(&signature).unwrap();
        let deserialized = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(signature, deserialized);
        assert_eq!(signature.tag(), deserialized.tag());
    }

    fn signature_hex_roundtrip(signature: Signature) {
        let hex_encoded = signature.to_hex();
        let decoded = Signature::from_hex(hex_encoded.as_bytes()).unwrap();
        assert_eq!(signature, decoded);
        assert_eq!(signature.tag(), decoded.tag());

        // Ensure malformed encoded version fails to decode.
        Signature::from_hex(&hex_encoded[..1]).unwrap_err();
        Signature::from_hex(&hex_encoded[1..]).unwrap_err();
    }

    fn hash<T: Hash>(data: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    fn check_ord_and_hash<T: Ord + PartialOrd + Hash + Copy>(low: T, high: T) {
        let low_copy = low;

        assert_eq!(hash(&low), hash(&low_copy));
        assert_ne!(hash(&low), hash(&high));

        assert_eq!(Ordering::Less, low.cmp(&high));
        assert_eq!(Some(Ordering::Less), low.partial_cmp(&high));

        assert_eq!(Ordering::Greater, high.cmp(&low));
        assert_eq!(Some(Ordering::Greater), high.partial_cmp(&low));

        assert_eq!(Ordering::Equal, low.cmp(&low_copy));
        assert_eq!(Some(Ordering::Equal), low.partial_cmp(&low_copy));
    }

    mod ed25519 {
        use super::*;

        const SECRET_KEY_LENGTH: usize = SecretKey::ED25519_LENGTH;
        const PUBLIC_KEY_LENGTH: usize = PublicKey::ED25519_LENGTH;
        const SIGNATURE_LENGTH: usize = Signature::ED25519_LENGTH;

        #[test]
        fn secret_key_serialization_roundtrip() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);
            super::secret_key_serialization_roundtrip(secret_key)
        }

        #[test]
        fn secret_key_from_bytes() {
            // Secret key should be `SecretKey::ED25519_LENGTH` bytes.
            let bytes = [0; SECRET_KEY_LENGTH + 1];
            assert!(SecretKey::ed25519_from_bytes(&bytes[..]).is_err());
            assert!(SecretKey::ed25519_from_bytes(&bytes[2..]).is_err());

            // Check the same bytes but of the right length succeeds.
            assert!(SecretKey::ed25519_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn secret_key_to_and_from_der() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);
            let der_encoded = secret_key.to_der().unwrap();
            secret_key_der_roundtrip(secret_key);

            // Check DER-encoded can be decoded by openssl.
            let _ = OpenSSLSecretKey::private_key_from_der(&der_encoded).unwrap();
        }

        #[test]
        fn secret_key_to_and_from_pem() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);
            secret_key_pem_roundtrip(secret_key);
        }

        #[test]
        fn known_secret_key_to_pem() {
            // Example values taken from https://tools.ietf.org/html/rfc8410#section-10.3
            const KNOWN_KEY_HEX: &str =
                "d4ee72dbf913584ad5b6d8f1f769f8ad3afe7c28cbf1d4fbe097a88f44755842";
            const KNOWN_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----"#;
            super::known_secret_key_to_pem(KNOWN_KEY_HEX, KNOWN_KEY_PEM, ED25519_TAG);
        }

        #[test]
        fn secret_key_to_and_from_file() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);
            secret_key_file_roundtrip(secret_key);
        }

        #[test]
        fn public_key_serialization_roundtrip() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_ed25519(&mut rng);
            super::public_key_serialization_roundtrip(public_key);
        }

        #[test]
        fn public_key_from_bytes() {
            // Public key should be `PublicKey::ED25519_LENGTH` bytes.  Create vec with an extra
            // byte.
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_ed25519(&mut rng);
            let bytes: Vec<u8> = iter::once(rng.gen())
                .chain(public_key.as_ref().iter().copied())
                .collect();

            assert!(PublicKey::ed25519_from_bytes(&bytes[..]).is_err());
            assert!(PublicKey::ed25519_from_bytes(&bytes[2..]).is_err());

            // Check the same bytes but of the right length succeeds.
            assert!(PublicKey::ed25519_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn public_key_to_and_from_der() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_ed25519(&mut rng);
            public_key_der_roundtrip(public_key);
        }

        #[test]
        fn public_key_to_and_from_pem() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_ed25519(&mut rng);
            public_key_pem_roundtrip(public_key);
        }

        #[test]
        fn known_public_key_to_pem() {
            // Example values taken from https://tools.ietf.org/html/rfc8410#section-10.1
            const KNOWN_KEY_HEX: &str =
                "19bf44096984cdfe8541bac167dc3b96c85086aa30b6b6cb0c5c38ad703166e1";
            const KNOWN_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAGb9ECWmEzf6FQbrBZ9w7lshQhqowtrbLDFw4rXAxZuE=
-----END PUBLIC KEY-----"#;
            super::known_public_key_to_pem(KNOWN_KEY_HEX, KNOWN_KEY_PEM);
        }

        #[test]
        fn public_key_to_and_from_file() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_ed25519(&mut rng);
            public_key_file_roundtrip(public_key);
        }

        #[test]
        fn public_key_to_and_from_hex() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_ed25519(&mut rng);
            public_key_hex_roundtrip(public_key);
        }

        #[test]
        fn signature_serialization_roundtrip() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);
            let public_key = PublicKey::from(&secret_key);
            let data = b"data";
            let signature = sign(data, &secret_key, &public_key, &mut rng);
            super::signature_serialization_roundtrip(signature);
        }

        #[test]
        fn signature_from_bytes() {
            // Signature should be < ~2^(252.5).
            let invalid_bytes = [255; SIGNATURE_LENGTH];
            assert!(Signature::ed25519_from_bytes(&invalid_bytes[..]).is_err());

            // Signature should be `Signature::ED25519_LENGTH` bytes.
            let bytes = [2; SIGNATURE_LENGTH + 1];
            assert!(Signature::ed25519_from_bytes(&bytes[..]).is_err());
            assert!(Signature::ed25519_from_bytes(&bytes[2..]).is_err());

            // Check the same bytes but of the right length succeeds.
            assert!(Signature::ed25519_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn signature_key_to_and_from_hex() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);
            let public_key = PublicKey::from(&secret_key);
            let data = b"data";
            let signature = sign(data, &secret_key, &public_key, &mut rng);
            signature_hex_roundtrip(signature);
        }

        #[test]
        fn public_key_traits() {
            let public_key_low = PublicKey::new_ed25519([1; PUBLIC_KEY_LENGTH]).unwrap();
            let public_key_high = PublicKey::new_ed25519([3; PUBLIC_KEY_LENGTH]).unwrap();
            check_ord_and_hash(public_key_low, public_key_high)
        }

        #[test]
        fn public_key_to_account_hash() {
            let public_key_high = PublicKey::new_ed25519([255; PUBLIC_KEY_LENGTH]).unwrap();
            assert_ne!(
                public_key_high.to_account_hash().as_ref(),
                public_key_high.as_ref()
            );
        }

        #[test]
        fn signature_traits() {
            let signature_low = Signature::new_ed25519([1; SIGNATURE_LENGTH]).unwrap();
            let signature_high = Signature::new_ed25519([3; SIGNATURE_LENGTH]).unwrap();
            check_ord_and_hash(signature_low, signature_high)
        }

        #[test]
        fn sign_and_verify() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);

            let public_key = PublicKey::from(&secret_key);
            let other_public_key = PublicKey::random_ed25519(&mut rng);
            let wrong_type_public_key = PublicKey::random_secp256k1(&mut rng);

            let message = b"message";
            let signature = sign(message, &secret_key, &public_key, &mut rng);

            assert!(verify(message, &signature, &public_key).is_ok());
            assert!(verify(message, &signature, &other_public_key).is_err());
            assert!(verify(message, &signature, &wrong_type_public_key).is_err());
            assert!(verify(&message[1..], &signature, &public_key).is_err());
        }

        #[test]
        fn account_hash_generation_is_consistent() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_ed25519(&mut rng);

            let public_key_node = PublicKey::from(&secret_key);
            let public_key_types: casper_types::PublicKey = public_key_node.into();

            let hash_node: AccountHash = public_key_node.to_account_hash();
            let hash_types: AccountHash = public_key_types.into();
            assert_eq!(hash_types, hash_node)
        }
    }

    mod secp256k1 {
        use super::*;

        const SECRET_KEY_LENGTH: usize = SecretKey::SECP256K1_LENGTH;
        const SIGNATURE_LENGTH: usize = Signature::SECP256K1_LENGTH;

        #[test]
        fn secret_key_serialization_roundtrip() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);
            super::secret_key_serialization_roundtrip(secret_key)
        }

        #[test]
        fn secret_key_from_bytes() {
            // Secret key should be `SecretKey::SECP256K1_LENGTH` bytes.
            let bytes = [0; SECRET_KEY_LENGTH + 1];
            assert!(SecretKey::secp256k1_from_bytes(&bytes[..]).is_err());
            assert!(SecretKey::secp256k1_from_bytes(&bytes[2..]).is_err());

            // Check the same bytes but of the right length succeeds.
            assert!(SecretKey::secp256k1_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn secret_key_to_and_from_der() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);
            secret_key_der_roundtrip(secret_key);
        }

        #[test]
        fn secret_key_to_and_from_pem() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);
            secret_key_pem_roundtrip(secret_key);
        }

        #[test]
        fn known_secret_key_to_pem() {
            // Example values taken from Python client.
            const KNOWN_KEY_HEX: &str =
                "bddfa9a30a01f5d22b50f63e75556d9959ee34efd77de7ba4ab8fbde6cea499c";
            const KNOWN_KEY_PEM: &str = r#"-----BEGIN EC PRIVATE KEY-----
MHQCAQEEIL3fqaMKAfXSK1D2PnVVbZlZ7jTv133nukq4+95s6kmcoAcGBSuBBAAK
oUQDQgAEQI6VJjFv0fje9IDdRbLMcv/XMnccnOtdkv+kBR5u4ISEAkuc2TFWQHX0
Yj9oTB9fx9+vvQdxJOhMtu46kGo0Uw==
-----END EC PRIVATE KEY-----"#;
            super::known_secret_key_to_pem(KNOWN_KEY_HEX, KNOWN_KEY_PEM, SECP256K1_TAG);
        }

        #[test]
        fn secret_key_to_and_from_file() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);
            secret_key_file_roundtrip(secret_key);
        }

        #[test]
        fn public_key_serialization_roundtrip() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            super::public_key_serialization_roundtrip(public_key);
        }

        #[test]
        fn public_key_from_bytes() {
            // Public key should be `PublicKey::SECP256K1_LENGTH` bytes.  Create vec with an extra
            // byte.
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            let bytes: Vec<u8> = iter::once(rng.gen())
                .chain(public_key.as_ref().iter().copied())
                .collect();

            assert!(PublicKey::secp256k1_from_bytes(&bytes[..]).is_err());
            assert!(PublicKey::secp256k1_from_bytes(&bytes[2..]).is_err());

            // Check the same bytes but of the right length succeeds.
            assert!(PublicKey::secp256k1_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn public_key_to_and_from_der() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            public_key_der_roundtrip(public_key);
        }

        #[test]
        fn public_key_to_and_from_pem() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            public_key_pem_roundtrip(public_key);
        }

        #[test]
        fn known_public_key_to_pem() {
            // Example values taken from Python client.
            const KNOWN_KEY_HEX: &str =
                "04408e9526316fd1f8def480dd45b2cc72ffd732771c9ceb5d92ffa4051e6ee08484024b9cd9315640\
                75f4623f684c1f5fc7dfafbd077124e84cb6ee3a906a3453";
            const KNOWN_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MFYwEAYHKoZIzj0CAQYFK4EEAAoDQgAEQI6VJjFv0fje9IDdRbLMcv/XMnccnOtd
kv+kBR5u4ISEAkuc2TFWQHX0Yj9oTB9fx9+vvQdxJOhMtu46kGo0Uw==
-----END PUBLIC KEY-----"#;
            super::known_public_key_to_pem(KNOWN_KEY_HEX, KNOWN_KEY_PEM);
        }

        #[test]
        fn public_key_to_and_from_file() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            public_key_file_roundtrip(public_key);
        }

        #[test]
        fn public_key_to_and_from_hex() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            public_key_hex_roundtrip(public_key);
        }

        #[test]
        fn signature_serialization_roundtrip() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);
            let public_key = PublicKey::from(&secret_key);
            let data = b"data";
            let signature = sign(data, &secret_key, &public_key, &mut rng);
            super::signature_serialization_roundtrip(signature);
        }

        #[test]
        fn signature_from_bytes() {
            // Signature should be `Signature::SECP256K1_LENGTH` bytes.
            let bytes = [2; SIGNATURE_LENGTH + 1];
            assert!(Signature::secp256k1_from_bytes(&bytes[..]).is_err());
            assert!(Signature::secp256k1_from_bytes(&bytes[2..]).is_err());

            // Check the same bytes but of the right length succeeds.
            assert!(Signature::secp256k1_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn signature_key_to_and_from_hex() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);
            let public_key = PublicKey::from(&secret_key);
            let data = b"data";
            let signature = sign(data, &secret_key, &public_key, &mut rng);
            signature_hex_roundtrip(signature);
        }

        #[test]
        fn public_key_traits() {
            let mut rng = TestRng::new();
            let public_key1 = PublicKey::random_secp256k1(&mut rng);
            let public_key2 = PublicKey::random_secp256k1(&mut rng);
            if public_key1.as_ref() < public_key2.as_ref() {
                check_ord_and_hash(public_key1, public_key2)
            } else {
                check_ord_and_hash(public_key2, public_key1)
            }
        }

        #[test]
        fn public_key_to_account_hash() {
            let mut rng = TestRng::new();
            let public_key = PublicKey::random_secp256k1(&mut rng);
            assert_ne!(public_key.to_account_hash().as_ref(), public_key.as_ref());
        }

        #[test]
        fn signature_traits() {
            let signature_low = Signature::new_secp256k1([1; SIGNATURE_LENGTH]).unwrap();
            let signature_high = Signature::new_secp256k1([3; SIGNATURE_LENGTH]).unwrap();
            check_ord_and_hash(signature_low, signature_high)
        }

        #[test]
        fn account_hash_generation_is_consistent() {
            let mut rng = TestRng::new();
            let secret_key = SecretKey::random_secp256k1(&mut rng);

            let public_key_node = PublicKey::from(&secret_key);
            let public_key_types: casper_types::PublicKey = public_key_node.into();

            let hash_node: AccountHash = public_key_node.to_account_hash();
            let hash_types: AccountHash = public_key_types.into();
            assert_eq!(hash_types, hash_node)
        }
    }

    #[test]
    fn public_key_traits() {
        let mut rng = TestRng::new();
        let ed25519_public_key = PublicKey::random_ed25519(&mut rng);
        let secp256k1_public_key = PublicKey::random_secp256k1(&mut rng);
        check_ord_and_hash(ed25519_public_key, secp256k1_public_key);
    }

    #[test]
    fn signature_traits() {
        let signature_low = Signature::new_ed25519([3; Signature::ED25519_LENGTH]).unwrap();
        let signature_high = Signature::new_secp256k1([1; Signature::SECP256K1_LENGTH]).unwrap();
        check_ord_and_hash(signature_low, signature_high)
    }

    #[test]
    fn sign_and_verify() {
        let mut rng = TestRng::new();
        let ed25519_secret_key = SecretKey::random_ed25519(&mut rng);
        let secp256k1_secret_key = SecretKey::random_secp256k1(&mut rng);

        let ed25519_public_key = PublicKey::from(&ed25519_secret_key);
        let secp256k1_public_key = PublicKey::from(&secp256k1_secret_key);

        let other_ed25519_public_key = PublicKey::random_ed25519(&mut rng);
        let other_secp256k1_public_key = PublicKey::random_secp256k1(&mut rng);

        let message = b"message";
        let ed25519_signature = sign(message, &ed25519_secret_key, &ed25519_public_key, &mut rng);
        let secp256k1_signature = sign(
            message,
            &secp256k1_secret_key,
            &secp256k1_public_key,
            &mut rng,
        );

        assert!(verify(message, &ed25519_signature, &ed25519_public_key).is_ok());
        assert!(verify(message, &secp256k1_signature, &secp256k1_public_key).is_ok());

        assert!(verify(message, &ed25519_signature, &other_ed25519_public_key).is_err());
        assert!(verify(message, &secp256k1_signature, &other_secp256k1_public_key).is_err());

        assert!(verify(message, &ed25519_signature, &secp256k1_public_key).is_err());
        assert!(verify(message, &secp256k1_signature, &ed25519_public_key).is_err());

        assert!(verify(&message[1..], &ed25519_signature, &ed25519_public_key).is_err());
        assert!(verify(&message[1..], &secp256k1_signature, &secp256k1_public_key).is_err());
    }

    #[test]
    fn should_construct_secp256k1_from_uncompressed_bytes() {
        let mut rng = TestRng::new();

        // Construct a secp256k1 secret key and use that to construct an uncompressed public key.
        let secp256k1_secret_key = {
            let mut bytes = [0u8; SecretKey::SECP256K1_LENGTH];
            rng.fill_bytes(&mut bytes[..]);
            k256::SecretKey::from_bytes(bytes.as_ref()).unwrap()
        };
        let uncompressed_public_key =
            k256::PublicKey::from_secret_key(&secp256k1_secret_key, false).unwrap();

        // Construct a CL secret key and public key from that (which will be a compressed key).
        let secret_key = SecretKey::Secp256k1(secp256k1_secret_key);
        let public_key = PublicKey::from(&secret_key);
        assert_eq!(public_key.as_ref().len(), PublicKey::SECP256K1_LENGTH);
        assert_ne!(
            uncompressed_public_key.as_bytes().len(),
            PublicKey::SECP256K1_LENGTH
        );

        // Construct a CL public key from the uncompressed one's bytes and ensure it's compressed.
        let from_uncompressed_bytes =
            PublicKey::secp256k1_from_bytes(uncompressed_public_key.as_bytes()).unwrap();
        assert_eq!(public_key, from_uncompressed_bytes);

        // Construct a CL public key from the uncompressed one's hex representation and ensure it's
        // compressed.
        let uncompressed_hex = format!("02{}", hex::encode(uncompressed_public_key.as_bytes()));
        let from_uncompressed_hex = PublicKey::from_hex(uncompressed_hex).unwrap();
        assert_eq!(public_key, from_uncompressed_hex);
    }
}
