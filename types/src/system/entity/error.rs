use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

use crate::{
    bytesrepr::{self, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// Errors which can occur while executing the Auction contract.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(strum::EnumIter))]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    /// A placeholder for a more clever name in the future.
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(0, Error::BuggerAll as u8);
    /// ```
    BuggerAll = 0,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::BuggerAll => formatter.write_str("Bugger all"),
        }
    }
}

impl CLTyped for Error {
    fn cl_type() -> CLType {
        CLType::U8
    }
}

// This error type is not intended to be used by third party crates.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct TryFromU8ForError(());

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<u8> for Error {
    type Error = TryFromU8ForError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            d if d == Self::BuggerAll as u8 => Ok(Self::BuggerAll),
            _ => Err(TryFromU8ForError(())),
        }
    }
}

impl ToBytes for Error {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let value = *self as u8;
        value.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}
