//! Cryptographic types and operations on them

mod asymmetric_key;
mod error;

use alloc::vec::Vec;

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

use crate::key::BLAKE2B_DIGEST_LENGTH;
#[cfg(any(feature = "std", test))]
pub use asymmetric_key::generate_ed25519_keypair;
#[cfg(any(feature = "testing", feature = "gens", test))]
pub use asymmetric_key::gens;
pub use asymmetric_key::{
    sign, verify, AsymmetricType, PublicKey, SecretKey, Signature, ED25519_TAG, SECP256K1_TAG,
    SYSTEM_ACCOUNT, SYSTEM_TAG,
};
pub use error::Error;
#[cfg(any(feature = "std", test))]
pub use error::ErrorExt;

#[doc(hidden)]
pub fn blake2b<T: AsRef<[u8]>>(data: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
    let mut result = [0; BLAKE2B_DIGEST_LENGTH];
    // NOTE: Assumed safe as `BLAKE2B_DIGEST_LENGTH` is a valid value for a hasher
    let mut hasher = VarBlake2b::new(BLAKE2B_DIGEST_LENGTH).expect("should create hasher");

    hasher.update(data);
    hasher.finalize_variable(|slice| {
        result.copy_from_slice(slice);
    });
    result
}

/// HashAlgoType
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub enum HashAlgoType {
    /// Blake2b
    Blake2b = 0,
    /// Blake3
    Blake3 = 1,
}

impl ToBytes for HashAlgoType {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        (*self as u8).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        1
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(*self as u8);
        Ok(())
    }
}

impl FromBytes for HashAlgoType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, bytes) = u8::from_bytes(bytes)?;
        match value {
            0 => Ok((HashAlgoType::Blake2b, bytes)),
            1 => Ok((HashAlgoType::Blake3, bytes)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

// Statics representing the discriminant values of the enum
static VARIANT_BLAKE_2B: u8 = HashAlgoType::Blake2b as u8;
static VARIANT_BLAKE_3: u8 = HashAlgoType::Blake3 as u8;
impl AsRef<u8> for HashAlgoType {
    fn as_ref(&self) -> &u8 {
        match self {
            HashAlgoType::Blake2b => &VARIANT_BLAKE_2B,
            HashAlgoType::Blake3 => &VARIANT_BLAKE_3,
        }
    }
}
