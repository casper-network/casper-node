use alloc::vec::Vec;
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

use crate::{
    addressable_entity::{AddKeyFailure, RemoveKeyFailure, UpdateKeyFailure},
    bytesrepr::{self, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// Errors which can occur while executing the Auction contract.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(strum::EnumIter))]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(0, Error::Serialization as u8);
    /// ```
    Serialization = 0,
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(1, Error::AddKey as u8);
    /// ```
    AddKey = 1,
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(2, Error::RemoveKey as u8);
    /// ```
    RemoveKey = 2,
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(3, Error::UpdateKey as u8);
    /// ```
    UpdateKey = 3,
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(4, Error::Storage as u8);
    /// ```
    Storage = 4,
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(5, Error::GasLimit as u8);
    /// ```
    GasLimit = 5,
    /// ```
    /// # use casper_types::system::entity::Error;
    /// assert_eq!(6, Error::CLValue as u8);
    /// ```
    CLValue = 6,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Self::Serialization => formatter.write_str("Serialization error"),
            Self::AddKey => formatter.write_str("Add key failure"),
            Self::RemoveKey => formatter.write_str("Remove key failure"),
            Self::UpdateKey => formatter.write_str("Update key failure"),
            Self::Storage => formatter.write_str("Storage error"),
            Self::GasLimit => formatter.write_str("Execution exceeded the gas limit"),
            Self::CLValue => formatter.write_str("CLValue error"),
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
            d if d == Self::Serialization as u8 => Ok(Self::Serialization),
            d if d == Self::AddKey as u8 => Ok(Self::AddKey),
            d if d == Self::RemoveKey as u8 => Ok(Self::RemoveKey),
            d if d == Self::UpdateKey as u8 => Ok(Self::UpdateKey),
            d if d == Self::Storage as u8 => Ok(Self::Storage),
            d if d == Self::GasLimit as u8 => Ok(Self::GasLimit),
            d if d == Self::CLValue as u8 => Ok(Self::CLValue),
            _ => Err(TryFromU8ForError(())),
        }
    }
}

impl From<AddKeyFailure> for Error {
    fn from(_error: AddKeyFailure) -> Self {
        Self::AddKey
    }
}

impl From<RemoveKeyFailure> for Error {
    fn from(_error: RemoveKeyFailure) -> Self {
        Self::RemoveKey
    }
}

impl From<UpdateKeyFailure> for Error {
    fn from(_error: UpdateKeyFailure) -> Self {
        Self::UpdateKey
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
