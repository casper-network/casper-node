//! Cryptographic types and operations on them

mod asymmetric_key;
mod error;

#[cfg(any(feature = "std", test))]
pub use asymmetric_key::generate_ed25519_keypair;
#[cfg(any(feature = "gens", test))]
pub use asymmetric_key::gens;
pub use asymmetric_key::{
    sign, verify, AsymmetricType, PublicKey, SecretKey, Signature, ED25519_TAG, SECP256K1_TAG,
    SYSTEM_ACCOUNT, SYSTEM_TAG,
};
pub use error::Error;
#[cfg(any(feature = "std", test))]
pub use error::ErrorExt;
