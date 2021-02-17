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
    /// Unable to obtain a key by its name.
    #[fail(display = "Missing key")]
    MissingKey = 8,
    /// Total supply not found.
    #[fail(display = "Total supply not found")]
    TotalSupplyNotFound = 9,
    /// Failed to record transfer.
    #[fail(display = "Failed to record transfer")]
    RecordTransferFailure = 10,
    /// Invalid attempt to reduce total supply.
    #[fail(display = "Invalid attempt to reduce total supply")]
    InvalidTotalSupplyReductionAttempt = 11,
    /// Failed to create new uref.
    #[fail(display = "Failed to create new uref")]
    NewURef = 12,
    /// Failed to put key.
    #[fail(display = "Failed to put key")]
    PutKey = 13,
    /// Failed to write local key.
    #[fail(display = "Failed to write local key")]
    WriteLocal = 14,
    /// Failed to create a [`crate::CLValue`].
    #[fail(display = "Failed to create a CLValue")]
    CLValue = 15,
    /// Failed to serialize data.
    #[fail(display = "Failed to serialize data")]
    Serialize = 16,
    /// Source and target purse [`crate::URef`]s are equal.
    #[fail(display = "Invalid target purse")]
    EqualSourceAndTarget = 17,

    // NOTE: These variants below will be removed once support for WASM system contracts will be
    // dropped.
    #[doc(hidden)]
    #[fail(display = "GasLimit")]
    GasLimit = 18,

    #[cfg(test)]
    #[doc(hidden)]
    #[fail(display = "Sentinel error")]
    Sentinel,
}

/// Used for testing; this should be guaranteed to be the maximum valid value of [`Error`] enum.
#[cfg(test)]
const MAX_ERROR_VALUE: u8 = Error::Sentinel as u8;

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
            d if d == Error::Storage as u8 => Ok(Error::Storage),
            d if d == Error::PurseNotFound as u8 => Ok(Error::PurseNotFound),
            d if d == Error::MissingKey as u8 => Ok(Error::MissingKey),
            d if d == Error::TotalSupplyNotFound as u8 => Ok(Error::TotalSupplyNotFound),
            d if d == Error::RecordTransferFailure as u8 => Ok(Error::RecordTransferFailure),
            d if d == Error::InvalidTotalSupplyReductionAttempt as u8 => {
                Ok(Error::InvalidTotalSupplyReductionAttempt)
            }
            d if d == Error::NewURef as u8 => Ok(Error::NewURef),
            d if d == Error::PutKey as u8 => Ok(Error::PutKey),
            d if d == Error::WriteLocal as u8 => Ok(Error::WriteLocal),
            d if d == Error::CLValue as u8 => Ok(Error::CLValue),
            d if d == Error::Serialize as u8 => Ok(Error::Serialize),
            d if d == Error::EqualSourceAndTarget as u8 => Ok(Error::EqualSourceAndTarget),
            d if d == Error::GasLimit as u8 => Ok(Error::GasLimit),
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

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::{Error, TryFromU8ForError, MAX_ERROR_VALUE};

    #[test]
    fn error_round_trips() {
        for i in 0..=u8::max_value() {
            match Error::try_from(i) {
                Ok(error) if i < MAX_ERROR_VALUE => assert_eq!(error as u8, i),
                Ok(error) => panic!(
                    "value of variant {} ({}) exceeds MAX_ERROR_VALUE ({})",
                    error, i, MAX_ERROR_VALUE
                ),
                Err(TryFromU8ForError(())) if i >= MAX_ERROR_VALUE => (),
                Err(TryFromU8ForError(())) => {
                    panic!("missing conversion from u8 to error value: {}", i)
                }
            }
        }
    }
}
