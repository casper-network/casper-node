use alloc::string::String;
use core::fmt::{self, Debug, Display, Formatter};

use base64::DecodeError;
use ed25519_dalek::ed25519::Error as SignatureError;
use hex::FromHexError; // Re-exported of signature::Error; used by both dalek and k256 libs

/// Cryptographic errors.
#[derive(Debug)]
pub enum Error {
    /// Error resulting from creating or using asymmetric key types.
    AsymmetricKey(String),

    /// Error resulting when decoding a type from a hex-encoded representation.
    FromHex(FromHexError),

    /// Error resulting when decoding a type from a base64 representation.
    FromBase64(DecodeError),

    /// Signature error.
    SignatureError(SignatureError),
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, formatter)
    }
}

impl From<FromHexError> for Error {
    fn from(error: FromHexError) -> Self {
        Error::FromHex(error)
    }
}

impl From<SignatureError> for Error {
    fn from(error: SignatureError) -> Self {
        Error::SignatureError(error)
    }
}
