use alloc::string::String;
use core::fmt::{self, Display, Formatter};
#[cfg(any(feature = "std", test))]
use std::error::Error as StdError;

#[cfg(feature = "datasize")]
use datasize::DataSize;
use ed25519_dalek::ed25519::Error as SignatureError;
#[cfg(any(feature = "std", test))]
use pem::PemError;
use serde::Serialize;
#[cfg(any(feature = "std", test))]
use thiserror::Error;

#[cfg(any(feature = "std", test))]
use crate::file_utils::{ReadFileError, WriteFileError};

/// Cryptographic errors.
#[derive(Clone, Eq, PartialEq, Debug, Serialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[non_exhaustive]
pub enum Error {
    /// Error resulting from creating or using asymmetric key types.
    AsymmetricKey(String),

    /// Error resulting when decoding a type from a hex-encoded representation.
    #[serde(with = "serde_helpers::Base16DecodeError")]
    #[cfg_attr(feature = "datasize", data_size(skip))]
    FromHex(base16::DecodeError),

    /// Error resulting when decoding a type from a base64 representation.
    #[serde(with = "serde_helpers::Base64DecodeError")]
    #[cfg_attr(feature = "datasize", data_size(skip))]
    FromBase64(base64::DecodeError),

    /// Signature error.
    SignatureError,

    /// Error trying to manipulate the system key.
    System(String),
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::AsymmetricKey(error_msg) => {
                write!(formatter, "asymmetric key error: {}", error_msg)
            }
            Error::FromHex(error) => {
                write!(formatter, "decoding from hex: {}", error)
            }
            Error::FromBase64(error) => {
                write!(formatter, "decoding from base 64: {}", error)
            }
            Error::SignatureError => {
                write!(formatter, "error in signature")
            }
            Error::System(error_msg) => {
                write!(formatter, "invalid operation on system key: {}", error_msg)
            }
        }
    }
}

impl From<base16::DecodeError> for Error {
    fn from(error: base16::DecodeError) -> Self {
        Error::FromHex(error)
    }
}

impl From<SignatureError> for Error {
    fn from(_error: SignatureError) -> Self {
        Error::SignatureError
    }
}

#[cfg(any(feature = "std", test))]
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::FromHex(error) => Some(error),
            Error::FromBase64(error) => Some(error),
            Error::AsymmetricKey(_) | Error::SignatureError | Error::System(_) => None,
        }
    }
}

/// Cryptographic errors extended with some additional variants.
#[cfg(any(feature = "std", test))]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ErrorExt {
    /// A basic crypto error.
    #[error("crypto error: {0:?}")]
    CryptoError(#[from] Error),

    /// Error trying to read a secret key.
    #[error("secret key load failed: {0}")]
    SecretKeyLoad(ReadFileError),

    /// Error trying to read a public key.
    #[error("public key load failed: {0}")]
    PublicKeyLoad(ReadFileError),

    /// Error trying to write a secret key.
    #[error("secret key save failed: {0}")]
    SecretKeySave(WriteFileError),

    /// Error trying to write a public key.
    #[error("public key save failed: {0}")]
    PublicKeySave(WriteFileError),

    /// Pem format error.
    #[error("pem error: {0}")]
    FromPem(String),

    /// DER format error.
    #[error("der error: {0}")]
    FromDer(#[from] derp::Error),

    /// Error in getting random bytes from the system's preferred random number source.
    #[error("failed to get random bytes: {0}")]
    GetRandomBytes(#[from] getrandom::Error),
}

#[cfg(any(feature = "std", test))]
impl From<PemError> for ErrorExt {
    fn from(error: PemError) -> Self {
        ErrorExt::FromPem(error.to_string())
    }
}

/// This module allows us to derive `Serialize` for the third party error types which don't
/// themselves derive it.
///
/// See <https://serde.rs/remote-derive.html> for more info.
#[allow(clippy::enum_variant_names)]
mod serde_helpers {
    use serde::Serialize;

    #[derive(Serialize)]
    #[serde(remote = "base16::DecodeError")]
    pub(super) enum Base16DecodeError {
        InvalidByte { index: usize, byte: u8 },
        InvalidLength { length: usize },
    }

    #[derive(Serialize)]
    #[serde(remote = "base64::DecodeError")]
    pub(super) enum Base64DecodeError {
        InvalidByte(usize, u8),
        InvalidLength,
        InvalidLastSymbol(usize, u8),
    }
}
