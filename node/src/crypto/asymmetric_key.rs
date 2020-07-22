//! Asymmetric-key types and functions.

use std::{
    cmp::Ordering,
    fmt::{self, Debug, Display, Formatter},
    fs,
    hash::{Hash, Hasher},
    path::Path,
};

use ed25519_dalek::{self as ed25519, ExpandedSecretKey};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use signature::Signature as Sig;

use super::{Error, Result};
use crate::crypto::hash::hash;
use casperlabs_types::account::AccountHash;

const ED25519_TAG: u8 = 0;
const ED25519: &str = "Ed25519";
const ED25519_LOWERCASE: &str = "ed25519";

/// A secret or private asymmetric key.
#[derive(Serialize, Deserialize)]
pub enum SecretKey {
    /// Ed25519 secret key.
    Ed25519(ed25519::SecretKey),
}

impl SecretKey {
    /// The length in bytes of an Ed25519 secret key,
    pub const ED25519_LENGTH: usize = ed25519::SECRET_KEY_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn new_ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Self {
        // safe to unwrap as `SecretKey::from_bytes` can only fail if the provided slice is the
        // wrong length.
        SecretKey::Ed25519(ed25519::SecretKey::from_bytes(&bytes).unwrap())
    }

    /// Constructs a new Ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Ok(SecretKey::Ed25519(ed25519::SecretKey::from_bytes(
            bytes.as_ref(),
        )?))
    }

    /// Constructs a new Ed25519 variant using the operating system's cryptographically secure
    /// random number generator.
    pub fn generate_ed25519() -> Self {
        let mut bytes = [0u8; Self::ED25519_LENGTH];
        getrandom::getrandom(&mut bytes[..]).expect("RNG failure!");
        SecretKey::new_ed25519(bytes)
    }

    /// Exposes the secret values of the key as a byte slice.
    pub fn as_secret_slice(&self) -> &[u8] {
        match self {
            SecretKey::Ed25519(secret_key) => secret_key.as_ref(),
        }
    }

    /// Attempt to read the secret key bytes from configured file path.
    pub fn from_file<P: AsRef<Path>>(file: P) -> Result<Self> {
        let payload = fs::read_to_string(file.as_ref()).map_err(|error| Error::ReadFile {
            file: file.as_ref().display().to_string(),
            error_msg: error.to_string(),
        })?;
        let pem = pem::parse(payload)?;
        Self::ed25519_from_bytes(pem.contents)
    }
}

impl Debug for SecretKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            SecretKey::Ed25519(_) => write!(formatter, "SecretKey::{}(...)", ED25519),
        }
    }
}

impl Display for SecretKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, formatter)
    }
}

/// A public asymmetric key.
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum PublicKey {
    /// Ed25519 public key.
    Ed25519(ed25519::PublicKey),
}

impl PublicKey {
    /// The length in bytes of an Ed25519 public key,
    pub const ED25519_LENGTH: usize = ed25519::PUBLIC_KEY_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn new_ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Result<Self> {
        Ok(PublicKey::Ed25519(ed25519::PublicKey::from_bytes(&bytes)?))
    }

    /// Constructs a new key from the algorithm name and a byte slice.
    pub fn key_from_algorithm_name_and_bytes<N: AsRef<str>, T: AsRef<[u8]>>(
        name: N,
        bytes: T,
    ) -> Result<Self> {
        match &*name.as_ref().trim().to_lowercase() {
            ED25519_LOWERCASE => Self::ed25519_from_bytes(bytes),
            _ => panic!("Invalid algorithm name!"),
        }
    }

    /// Constructs a new Ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        Ok(PublicKey::Ed25519(ed25519::PublicKey::from_bytes(
            bytes.as_ref(),
        )?))
    }

    fn tag(&self) -> u8 {
        match self {
            PublicKey::Ed25519(_) => ED25519_TAG,
        }
    }

    fn variant_name(&self) -> &str {
        match self {
            PublicKey::Ed25519(_) => ED25519,
        }
    }

    /// Creates an `AccountHash` from a given `PublicKey` instance.
    pub(crate) fn to_account_hash(&self) -> AccountHash {
        // As explained here:
        // https://casperlabs.atlassian.net/wiki/spaces/EN/pages/446431524/Design+for+supporting+multiple+signature+algorithms.
        let (algorithm_name, pk_bytes) = match self {
            PublicKey::Ed25519(bytes) => (ED25519_LOWERCASE, bytes.as_ref()),
        };
        // Prepare preimage based on the public key parameters
        let preimage = {
            let mut data = Vec::with_capacity(algorithm_name.len() + pk_bytes.len() + 1);
            data.extend(algorithm_name.as_bytes());
            data.push(0x00);
            data.extend(pk_bytes);
            data
        };
        // Hash the preimage data using blake2b256 and return it
        let digest = hash(&preimage);
        AccountHash::new(digest.to_bytes())
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        match self {
            PublicKey::Ed25519(public_key) => public_key.as_ref(),
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

// This is inside a private module so that the generated `BigArray` does not form part of this
// crate's public API, and hence also doesn't appear in the rustdocs.
mod big_array {
    use serde_big_array::big_array;

    big_array! { BigArray; }
}

/// A signature of given data.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Signature {
    /// Ed25519 signature.
    //
    // This is held as a byte array rather than an `ed25519_dalek::Signature` as that type doesn't
    // implement `AsRef` amongst other common traits.  In order to implement these common traits,
    // it is convenient and cheap to use `signature.as_ref()`.
    Ed25519(#[serde(with = "big_array::BigArray")] [u8; ed25519::SIGNATURE_LENGTH]),
}

impl Signature {
    /// The length in bytes of an Ed25519 signature,
    pub const ED25519_LENGTH: usize = ed25519::SIGNATURE_LENGTH;

    /// Constructs a new Ed25519 variant from a byte array.
    pub fn new_ed25519(bytes: [u8; Self::ED25519_LENGTH]) -> Result<Self> {
        let signature = ed25519::Signature::from_bytes(&bytes)?;
        Ok(Signature::Ed25519(signature.to_bytes()))
    }

    /// Constructs a new Ed25519 variant from a byte slice.
    pub fn ed25519_from_bytes<T: AsRef<[u8]>>(bytes: T) -> Result<Self> {
        let signature = ed25519::Signature::from_bytes(bytes.as_ref())?;
        Ok(Signature::Ed25519(signature.to_bytes()))
    }

    fn tag(&self) -> u8 {
        match self {
            Signature::Ed25519(_) => ED25519_TAG,
        }
    }

    fn variant_name(&self) -> &str {
        match self {
            Signature::Ed25519(_) => ED25519,
        }
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        match self {
            Signature::Ed25519(signature) => signature.as_ref(),
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

/// Signs the given message using the given key pair.
pub fn sign<T: AsRef<[u8]>>(
    message: T,
    secret_key: &SecretKey,
    public_key: &PublicKey,
) -> Signature {
    match (secret_key, public_key) {
        (SecretKey::Ed25519(secret_key), PublicKey::Ed25519(public_key)) => {
            let expanded_secret_key = ExpandedSecretKey::from(secret_key);
            let signature = expanded_secret_key.sign(message.as_ref(), public_key);
            Signature::Ed25519(signature.to_bytes())
        }
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
                &ed25519::Signature::from_bytes(signature)?,
            )
            .map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod ed25519 {
        use std::{
            cmp::Ordering,
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        use super::*;

        const SECRET_KEY_LENGTH: usize = SecretKey::ED25519_LENGTH;
        const PUBLIC_KEY_LENGTH: usize = PublicKey::ED25519_LENGTH;
        const SIGNATURE_LENGTH: usize = Signature::ED25519_LENGTH;

        #[test]
        fn secret_key_from_bytes() {
            // secret key should be `SecretKey::ED25519_LENGTH` bytes
            let bytes = [0; SECRET_KEY_LENGTH + 1];
            assert!(SecretKey::ed25519_from_bytes(&bytes[..]).is_err());
            assert!(SecretKey::ed25519_from_bytes(&bytes[2..]).is_err());

            // check the same bytes but of the right length succeeds
            assert!(SecretKey::ed25519_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn public_key_from_bytes() {
            // public key should be `PublicKey::ED25519_LENGTH` bytes
            let bytes = [1; PUBLIC_KEY_LENGTH + 1];
            assert!(PublicKey::ed25519_from_bytes(&bytes[..]).is_err());
            assert!(PublicKey::ed25519_from_bytes(&bytes[2..]).is_err());

            // check the same bytes but of the right length succeeds
            assert!(PublicKey::ed25519_from_bytes(&bytes[1..]).is_ok());
        }

        #[test]
        fn signature_from_bytes() {
            // signature should be < ~2^(252.5)
            let invalid_bytes = [255; SIGNATURE_LENGTH];
            assert!(Signature::ed25519_from_bytes(&invalid_bytes[..]).is_err());

            // signature should be `Signature::ED25519_LENGTH` bytes
            let bytes = [2; SIGNATURE_LENGTH + 1];
            assert!(Signature::ed25519_from_bytes(&bytes[..]).is_err());
            assert!(Signature::ed25519_from_bytes(&bytes[2..]).is_err());

            // check the same bytes but of the right length succeeds
            assert!(Signature::ed25519_from_bytes(&bytes[1..]).is_ok());
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
            let secret_key = SecretKey::generate_ed25519();

            let public_key = PublicKey::from(&secret_key);
            let other_public_key = PublicKey::from(&SecretKey::generate_ed25519());

            let message = b"message";
            let signature = sign(message, &secret_key, &public_key);

            assert!(verify(message, &signature, &public_key).is_ok());
            assert!(verify(message, &signature, &other_public_key).is_err());
            assert!(verify(&message[1..], &signature, &public_key).is_err());
        }
    }
}
