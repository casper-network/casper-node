use core::array::TryFromSliceError;

use alloc::string::String;
use core::{
    fmt::{self, Debug, Display, Formatter},
    num::ParseIntError,
};

/// Error while parsing message hashes from string.
#[derive(Debug)]
#[non_exhaustive]
pub enum FromStrError {
    /// The prefix is invalid.
    InvalidPrefix,
    /// No message index at the end of the string.
    MissingMessageIndex,
    /// String not formatted correctly.
    Formatting,
    /// Cannot parse entity hash.
    EntityHashParseError(String),
    /// Cannot parse message topic hash.
    MessageTopicParseError(String),
    /// Failed to decode address portion of URef.
    Hex(base16::DecodeError),
    /// Failed to parse an int.
    Int(ParseIntError),
    /// The slice is the wrong length.
    Length(TryFromSliceError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<ParseIntError> for FromStrError {
    fn from(error: ParseIntError) -> Self {
        FromStrError::Int(error)
    }
}

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Length(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix => {
                write!(f, "prefix is invalid")
            }
            FromStrError::MissingMessageIndex => {
                write!(f, "no message index found at the end of the string")
            }
            FromStrError::Formatting => {
                write!(f, "string not properly formatted")
            }
            FromStrError::EntityHashParseError(err) => {
                write!(f, "could not parse entity hash: {}", err)
            }
            FromStrError::MessageTopicParseError(err) => {
                write!(f, "could not parse topic hash: {}", err)
            }
            FromStrError::Hex(error) => {
                write!(f, "failed to decode address portion from hex: {}", error)
            }
            FromStrError::Int(error) => write!(f, "failed to parse an int: {}", error),
            FromStrError::Length(error) => write!(f, "address portion is wrong length: {}", error),
        }
    }
}
