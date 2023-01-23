use alloc::string::String;
use core::fmt::Debug;
#[cfg(not(any(feature = "std", test)))]
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use ed25519_dalek::ed25519::Error as SignatureError;
#[cfg(any(feature = "std", test))]
use pem::PemError;
#[cfg(any(feature = "std", test))]
use thiserror::Error;

#[cfg(any(feature = "std", test))]
use crate::file_utils::{ReadFileError, WriteFileError};

/// Cryptographic errors.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(any(feature = "std", test), derive(Error))]
#[non_exhaustive]
pub enum Error {
    /// Error resulting from creating or using asymmetric key types.
    #[cfg_attr(any(feature = "std", test), error("asymmetric key error: {0}"))]
    AsymmetricKey(String),

    /// Error resulting when decoding a type from a hex-encoded representation.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    #[cfg_attr(any(feature = "std", test), error("parsing from hex: {0}"))]
    FromHex(base16::DecodeError),

    /// Error resulting when decoding a type from a base64 representation.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    #[cfg_attr(any(feature = "std", test), error("decoding error: {0}"))]
    FromBase64(base64::DecodeError),

    /// Signature error.
    #[cfg_attr(any(feature = "std", test), error("error in signature"))]
    SignatureError,

    /// Error trying to manipulate the system key.
    #[cfg_attr(
        any(feature = "std", test),
        error("invalid operation on system key: {0}")
    )]
    System(String),
}

#[cfg(not(any(feature = "std", test)))]
impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, formatter)
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
