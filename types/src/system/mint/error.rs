//! Home of the Mint contract's [`enum@Error`] type.

use alloc::vec::Vec;
use core::{
    convert::{TryFrom, TryInto},
    fmt::{self, Display, Formatter},
};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// Errors which can occur while executing the Mint contract.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    /// Insufficient funds to complete the transfer.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(0, Error::InsufficientFunds as u8);
    /// ```
    InsufficientFunds = 0,
    /// Source purse not found.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(1, Error::SourceNotFound as u8);
    /// ```
    SourceNotFound = 1,
    /// Destination purse not found.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(2, Error::DestNotFound as u8);
    /// ```
    DestNotFound = 2,
    /// The given [`URef`](crate::URef) does not reference the account holder's purse, or such a
    /// `URef` does not have the required [`AccessRights`](crate::AccessRights).
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(3, Error::InvalidURef as u8);
    /// ```
    InvalidURef = 3,
    /// The source purse is not writeable (see [`URef::is_writeable`](crate::URef::is_writeable)),
    /// or the destination purse is not addable (see
    /// [`URef::is_addable`](crate::URef::is_addable)).
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(4, Error::InvalidAccessRights as u8);
    /// ```
    InvalidAccessRights = 4,
    /// Tried to create a new purse with a non-zero initial balance.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(5, Error::InvalidNonEmptyPurseCreation as u8);
    /// ```
    InvalidNonEmptyPurseCreation = 5,
    /// Failed to read from local or global storage.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(6, Error::Storage as u8);
    /// ```
    Storage = 6,
    /// Purse not found while trying to get balance.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(7, Error::PurseNotFound as u8);
    /// ```
    PurseNotFound = 7,
    /// Unable to obtain a key by its name.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(8, Error::MissingKey as u8);
    /// ```
    MissingKey = 8,
    /// Total supply not found.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(9, Error::TotalSupplyNotFound as u8);
    /// ```
    TotalSupplyNotFound = 9,
    /// Failed to record transfer.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(10, Error::RecordTransferFailure as u8);
    /// ```
    RecordTransferFailure = 10,
    /// Invalid attempt to reduce total supply.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(11, Error::InvalidTotalSupplyReductionAttempt as u8);
    /// ```
    InvalidTotalSupplyReductionAttempt = 11,
    /// Failed to create new uref.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(12, Error::NewURef as u8);
    /// ```
    NewURef = 12,
    /// Failed to put key.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(13, Error::PutKey as u8);
    /// ```
    PutKey = 13,
    /// Failed to write to dictionary.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(14, Error::WriteDictionary as u8);
    /// ```
    WriteDictionary = 14,
    /// Failed to create a [`crate::CLValue`].
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(15, Error::CLValue as u8);
    /// ```
    CLValue = 15,
    /// Failed to serialize data.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(16, Error::Serialize as u8);
    /// ```
    Serialize = 16,
    /// Source and target purse [`crate::URef`]s are equal.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(17, Error::EqualSourceAndTarget as u8);
    /// ```
    EqualSourceAndTarget = 17,
    /// An arithmetic overflow has occurred.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(18, Error::ArithmeticOverflow as u8);
    /// ```
    ArithmeticOverflow = 18,

    // NOTE: These variants below will be removed once support for WASM system contracts will be
    // dropped.
    #[doc(hidden)]
    GasLimit = 19,

    /// Raised when an entry point is called from invalid account context.
    InvalidContext = 20,

    /// Session code tried to transfer more CSPR than user approved.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(21, Error::UnapprovedSpendingAmount as u8);
    UnapprovedSpendingAmount = 21,

    /// Failed to transfer tokens on a private chain.
    /// ```
    /// # use casper_types::system::mint::Error;
    /// assert_eq!(22, Error::DisabledUnrestrictedTransfers as u8);
    DisabledUnrestrictedTransfers = 22,

    #[cfg(test)]
    #[doc(hidden)]
    Sentinel,
}

/// Used for testing; this should be guaranteed to be the maximum valid value of [`Error`] enum.
#[cfg(test)]
const MAX_ERROR_VALUE: u8 = Error::Sentinel as u8;

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
            d if d == Error::WriteDictionary as u8 => Ok(Error::WriteDictionary),
            d if d == Error::CLValue as u8 => Ok(Error::CLValue),
            d if d == Error::Serialize as u8 => Ok(Error::Serialize),
            d if d == Error::EqualSourceAndTarget as u8 => Ok(Error::EqualSourceAndTarget),
            d if d == Error::ArithmeticOverflow as u8 => Ok(Error::ArithmeticOverflow),
            d if d == Error::GasLimit as u8 => Ok(Error::GasLimit),
            d if d == Error::InvalidContext as u8 => Ok(Error::InvalidContext),
            d if d == Error::UnapprovedSpendingAmount as u8 => Ok(Error::UnapprovedSpendingAmount),
            d if d == Error::DisabledUnrestrictedTransfers as u8 => {
                Ok(Error::DisabledUnrestrictedTransfers)
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

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::InsufficientFunds => formatter.write_str("Insufficient funds"),
            Error::SourceNotFound => formatter.write_str("Source not found"),
            Error::DestNotFound => formatter.write_str("Destination not found"),
            Error::InvalidURef => formatter.write_str("Invalid URef"),
            Error::InvalidAccessRights => formatter.write_str("Invalid AccessRights"),
            Error::InvalidNonEmptyPurseCreation => {
                formatter.write_str("Invalid non-empty purse creation")
            }
            Error::Storage => formatter.write_str("Storage error"),
            Error::PurseNotFound => formatter.write_str("Purse not found"),
            Error::MissingKey => formatter.write_str("Missing key"),
            Error::TotalSupplyNotFound => formatter.write_str("Total supply not found"),
            Error::RecordTransferFailure => formatter.write_str("Failed to record transfer"),
            Error::InvalidTotalSupplyReductionAttempt => {
                formatter.write_str("Invalid attempt to reduce total supply")
            }
            Error::NewURef => formatter.write_str("Failed to create new uref"),
            Error::PutKey => formatter.write_str("Failed to put key"),
            Error::WriteDictionary => formatter.write_str("Failed to write dictionary"),
            Error::CLValue => formatter.write_str("Failed to create a CLValue"),
            Error::Serialize => formatter.write_str("Failed to serialize data"),
            Error::EqualSourceAndTarget => formatter.write_str("Invalid target purse"),
            Error::ArithmeticOverflow => formatter.write_str("Arithmetic overflow has occurred"),
            Error::GasLimit => formatter.write_str("GasLimit"),
            Error::InvalidContext => formatter.write_str("Invalid context"),
            Error::UnapprovedSpendingAmount => formatter.write_str("Unapproved spending amount"),
            Error::DisabledUnrestrictedTransfers => {
                formatter.write_str("Disabled unrestricted transfers")
            }
            #[cfg(test)]
            Error::Sentinel => formatter.write_str("Sentinel error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, TryFromU8ForError, MAX_ERROR_VALUE};

    #[test]
    fn error_round_trips() {
        for i in 0..=u8::max_value() {
            match Error::try_from(i) {
                Ok(error) if i < MAX_ERROR_VALUE => assert_eq!(error as u8, i),
                Ok(error) => panic!(
                    "value of variant {:?} ({}) exceeds MAX_ERROR_VALUE ({})",
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
