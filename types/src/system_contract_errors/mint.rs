//! Home of the Mint contract's [`Error`] type.

use alloc::{fmt, vec::Vec};
use core::convert::{TryFrom, TryInto};

use failure::Fail;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    AccessRights, CLType, CLTyped,
};

/// Errors which can occur while executing the Mint contract.
#[derive(Fail, Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    /// Insufficient funds to complete the transfer.
    #[fail(display = "Insufficient funds")]
    InsufficientFunds = 0,
    /// Source purse not found.
    #[fail(display = "Source not found")]
    SourceNotFound = 1,
    /// Destination purse not found.
    #[fail(display = "Destination not found")]
    DestNotFound = 2,
    /// See [`PurseError::InvalidURef`].
    #[fail(display = "Invalid URef")]
    InvalidURef = 3,
    /// See [`PurseError::InvalidAccessRights`].
    #[fail(display = "Invalid AccessRights")]
    InvalidAccessRights = 4,
    /// Tried to create a new purse with a non-zero initial balance.
    #[fail(display = "Invalid non-empty purse creation")]
    InvalidNonEmptyPurseCreation = 5,
    /// Failed to read from local or global storage.
    #[fail(display = "Storage error")]
    Storage = 6,
    /// Purse not found while trying to get balance.
    #[fail(display = "Purse not found")]
    PurseNotFound = 7,
}

impl From<PurseError> for Error {
    fn from(purse_error: PurseError) -> Error {
        match purse_error {
            PurseError::InvalidURef => Error::InvalidURef,
            PurseError::InvalidAccessRights(_) => {
                // This one does not carry state from PurseError to the new Error enum. The reason
                // is that Error is supposed to be simple in serialization and deserialization, so
                // extra state is currently discarded.
                Error::InvalidAccessRights
            }
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
pub struct TryFromU8ForError(());

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl TryFrom<u8> for Error {
    type Error = TryFromU8ForError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            d if d == Error::InsufficientFunds as u8 => Ok(Error::InsufficientFunds),
            d if d == Error::SourceNotFound as u8 => Ok(Error::SourceNotFound),
            d if d == Error::DestNotFound as u8 => Ok(Error::DestNotFound),
            d if d == Error::InvalidURef as u8 => Ok(Error::InvalidURef),
            d if d == Error::InvalidAccessRights as u8 => Ok(Error::InvalidAccessRights),
            d if d == Error::InvalidNonEmptyPurseCreation as u8 => {
                Ok(Error::InvalidNonEmptyPurseCreation)
            }
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

impl FromBytes for Error {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, rem): (u8, _) = FromBytes::from_bytes(bytes)?;
        let error: Error = value
            .try_into()
            // In case an Error variant is unable to be determined it would return an
            // Error::Formatting as if its unable to be correctly deserialized.
            .map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((error, rem))
    }
}

/// Errors relating to validity of source or destination purses.
#[derive(Debug, Copy, Clone)]
pub enum PurseError {
    /// The given [`URef`](crate::URef) does not reference the account holder's purse, or such a
    /// [`URef`](crate::URef) does not have the required [`AccessRights`].
    InvalidURef,
    /// The source purse is not writeable (see [`URef::is_writeable`](crate::URef::is_writeable)),
    /// or the destination purse is not addable (see
    /// [`URef::is_addable`](crate::URef::is_addable)).
    InvalidAccessRights(Option<AccessRights>),
}

impl fmt::Display for PurseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            PurseError::InvalidURef => write!(f, "invalid uref"),
            PurseError::InvalidAccessRights(maybe_access_rights) => {
                write!(f, "invalid access rights: {:?}", maybe_access_rights)
            }
        }
    }
}
