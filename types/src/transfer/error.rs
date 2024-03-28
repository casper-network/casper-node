use core::{
    array::TryFromSliceError,
    fmt::{self, Debug, Display, Formatter},
};
#[cfg(feature = "std")]
use std::error::Error as StdError;

/// Error returned when decoding a `TransferAddr` from a formatted string.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TransferFromStrError {
    /// The prefix is invalid.
    InvalidPrefix,
    /// The address is not valid hex.
    Hex(base16::DecodeError),
    /// The slice is the wrong length.
    Length(TryFromSliceError),
}

impl From<base16::DecodeError> for TransferFromStrError {
    fn from(error: base16::DecodeError) -> Self {
        TransferFromStrError::Hex(error)
    }
}

impl From<TryFromSliceError> for TransferFromStrError {
    fn from(error: TryFromSliceError) -> Self {
        TransferFromStrError::Length(error)
    }
}

impl Display for TransferFromStrError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransferFromStrError::InvalidPrefix => {
                write!(formatter, "transfer addr prefix is invalid",)
            }
            TransferFromStrError::Hex(error) => {
                write!(
                    formatter,
                    "failed to decode address portion of transfer addr from hex: {}",
                    error
                )
            }
            TransferFromStrError::Length(error) => write!(
                formatter,
                "address portion of transfer addr is wrong length: {}",
                error
            ),
        }
    }
}

#[cfg(feature = "std")]
impl StdError for TransferFromStrError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            TransferFromStrError::InvalidPrefix => None,
            TransferFromStrError::Hex(error) => Some(error),
            TransferFromStrError::Length(error) => Some(error),
        }
    }
}
