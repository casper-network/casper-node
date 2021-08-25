//! Cryptographic types and functions.

mod asymmetric_key;
mod asymmetric_key_ext;
mod error;
pub mod hash;

#[cfg(test)]
pub(crate) use asymmetric_key::generate_ed25519_keypair;
pub(crate) use asymmetric_key::{sign, verify};
pub use asymmetric_key_ext::AsymmetricKeyExt;
pub use error::Error;
pub(crate) use error::Result;
