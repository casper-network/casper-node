use core::{
    array::TryFromSliceError,
    fmt::{self, Display, Formatter},
};

/// Error returned when decoding an `AccountHash` from a formatted string.
#[derive(Debug)]
#[non_exhaustive]
pub enum FromStrError {
    /// The prefix is invalid.
    InvalidPrefix,
    /// The hash is not valid hex.
    Hex(base16::DecodeError),
    /// The hash is the wrong length.
    Hash(TryFromSliceError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Hash(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix => write!(f, "prefix is not 'account-hash-'"),
            FromStrError::Hex(error) => {
                write!(f, "failed to decode address portion from hex: {}", error)
            }
            FromStrError::Hash(error) => write!(f, "address portion is wrong length: {}", error),
        }
    }
}
/// Associated error type of `TryFrom<&[u8]>` for [`AccountHash`](super::AccountHash).
#[derive(Debug)]
pub struct TryFromSliceForAccountHashError(());
