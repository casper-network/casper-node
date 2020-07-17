use std::result;

use ed25519_dalek::SignatureError;
use hex::FromHexError;
use thiserror::Error;

/// A specialized `std::result::Result` type for cryptographic errors.
pub type Result<T> = result::Result<T, Error>;

/// Cryptographic errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Error resulting from creating or using asymmetric key types.
    #[error("asymmetric key error: {0}")]
    AsymmetricKey(SignatureError),
    /// Error resulting when decoding a type from a hex-encoded representation.
    #[error("parsing from hex: {0}")]
    FromHex(#[from] FromHexError),
}

impl From<SignatureError> for Error {
    fn from(signature_error: SignatureError) -> Self {
        Error::AsymmetricKey(signature_error)
    }
}
