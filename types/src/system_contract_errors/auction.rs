//! Home of the Auction contract's [`Error`] type.
use alloc::vec::Vec;
use core::result::Result as StdResult;

use failure::Fail;

use crate::{bytesrepr, CLType, CLTyped};
use bytesrepr::{ToBytes, U8_SERIALIZED_LENGTH};

/// Errors which can occur while executing the Auction contract.
#[derive(Fail, Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    /// Unable to find named key in the contract's named keys.
    #[fail(display = "Missing key")]
    MissingKey = 0,
    /// Given named key contains invalid variant.
    #[fail(display = "Invalid key variant")]
    InvalidKeyVariant = 1,
    /// Value under an uref does not exist. This means the installer contract didn't work properly.
    #[fail(display = "Missing value")]
    MissingValue = 2,
    /// ABI serialization issue while reading or writing.
    #[fail(display = "Serialization error")]
    Serialization = 3,
    /// Triggered when contract was unable to transfer desired amount of tokens.
    #[fail(display = "Transfer error")]
    Transfer = 4,
    /// User passed invalid quantity of tokens which might result in wrong values after calculation.
    #[fail(display = "Invalid quantity")]
    InvalidQuantity = 5,
    /// Unable to find a bid by account hash in `active_bids` map.
    #[fail(display = "Bid not found")]
    BidNotFound = 6,
    /// Validator's account hash was not found in the map.
    #[fail(display = "Validator not found")]
    ValidatorNotFound = 7,
    /// Delegator's account hash was not found in the map.
    #[fail(display = "Delegator not found")]
    DelegatorNotFound = 8,
    /// Storage problem.
    #[fail(display = "Storage error")]
    Storage = 9,
    /// Raised when system is unable to bond.
    #[fail(display = "Bonding error")]
    Bonding = 10,
    /// Raised when system is unable to unbond.
    #[fail(display = "Unbonding error")]
    Unbonding = 11,
    /// Raised when Mint contract is unable to release founder stake.
    #[fail(display = "Unable to release founder stake")]
    ReleaseFounderStake = 12,
    /// Raised when the system is unable to determine purse balance.
    #[fail(display = "Unable to get purse balance")]
    GetBalance = 13,
    /// Raised when an entry point is called from invalid account context.
    #[fail(display = "Invalid context")]
    InvalidContext = 14,
    /// Raised whenever a validator's funds are still locked in but an attempt to withdraw was made.
    #[fail(display = "Validator's funds are locked")]
    ValidatorFundsLocked = 15,
}

impl CLTyped for Error {
    fn cl_type() -> CLType {
        CLType::U8
    }
}

impl ToBytes for Error {
    fn to_bytes(&self) -> StdResult<Vec<u8>, bytesrepr::Error> {
        let value = *self as u8;
        value.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl From<bytesrepr::Error> for Error {
    fn from(_: bytesrepr::Error) -> Self {
        Error::Serialization
    }
}

/// An alias for `Result<T, auction::Error>`.
pub type Result<T> = StdResult<T, Error>;
