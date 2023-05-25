//! Cryptographic types and operations on them

mod asymmetric_key;
mod error;

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};

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
