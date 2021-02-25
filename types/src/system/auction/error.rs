//! Home of the Auction contract's [`Error`] type.
use alloc::vec::Vec;
use core::{
    convert::{TryFrom, TryInto},
    result,
};

use failure::Fail;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

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
    /// Triggered when contract was unable to transfer desired amount of tokens into a bid purse.
    #[fail(display = "Transfer to bid purse error")]
    TransferToBidPurse = 4,
    /// User passed invalid amount of tokens which might result in wrong values after calculation.
    #[fail(display = "Invalid amount")]
    InvalidAmount = 5,
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
    /// Raised whenever a validator's funds are still locked in but an attempt to withdraw was
    /// made.
    #[fail(display = "Validator's funds are locked")]
    ValidatorFundsLocked = 15,
    /// Raised when caller is not the system account.
    #[fail(display = "Function must be called by system account")]
    InvalidCaller = 16,
    /// Raised when function is supplied a public key that does match the caller's.
    #[fail(display = "Supplied public key does not match caller's public key")]
    InvalidPublicKey = 17,
    /// Validator is not not bonded.
    #[fail(display = "Validator's bond not found")]
    BondNotFound = 18,
    /// Unable to create purse.
    #[fail(display = "Unable to create purse")]
    CreatePurseFailed = 19,
    /// Attempted to unbond an amount which was too large.
    #[fail(display = "Unbond is too large")]
    UnbondTooLarge = 20,
    /// Attempted to bond with a stake which was too small.
    #[fail(display = "Bond is too small")]
    BondTooSmall = 21,
    /// Raised when rewards are to be distributed to delegators, but the validator has no
    /// delegations.
    #[fail(display = "Validators has not received any delegations")]
    MissingDelegations = 22,
    /// The validators returned by the consensus component should match
    /// current era validators when distributing rewards.
    #[fail(display = "Mismatched era validator sets to distribute rewards")]
    MismatchedEraValidators = 23,
    /// Failed to mint reward tokens.
    #[fail(display = "Failed to mint rewards")]
    MintReward = 24,
    /// Invalid number of validator slots.
    #[fail(display = "Invalid number of validator slots")]
    InvalidValidatorSlotsValue = 25,
    /// Failed to reduce total supply.
    #[fail(display = "Failed to reduce total supply")]
    MintReduceTotalSupply = 26,
    /// Triggered when contract was unable to transfer desired amount of tokens into a delegators
    /// purse.
    #[fail(display = "Transfer to delegators purse error")]
    TransferToDelegatorPurse = 27,
    /// Triggered when contract was unable to perform a transfer to distribute validators reward.
    #[fail(display = "Reward transfer to validator error")]
    ValidatorRewardTransfer = 28,
    /// Triggered when contract was unable to perform a transfer to distribute delegators rewards.
    #[fail(display = "Rewards transfer to delegator error")]
    DelegatorRewardTransfer = 29,
    /// Failed to transfer desired amount while withdrawing delegators reward.
    #[fail(display = "Withdraw delegator reward error")]
    WithdrawDelegatorReward = 30,
    /// Failed to transfer desired amount while withdrawing validators reward.
    #[fail(display = "Withdraw validator reward error")]
    WithdrawValidatorReward = 31,
    /// Failed to transfer desired amount into unbonding purse.
    #[fail(display = "Transfer to unbonding purse error")]
    TransferToUnbondingPurse = 32,
    /// Failed to record era info.
    #[fail(display = "Record era info error")]
    RecordEraInfo = 33,
    /// Failed to create a [`crate::CLValue`].
    #[fail(display = "CLValue error")]
    CLValue = 34,
    /// Missing seigniorage recipients for given era.
    #[fail(display = "Missing seigniorage recipients for given era")]
    MissingSeigniorageRecipients = 35,
    /// Failed to transfer funds.
    #[fail(display = "Transfer error")]
    Transfer = 36,
    /// Delegation rate exceeds rate.
    #[fail(display = "Delegation rate too large")]
    DelegationRateTooLarge = 37,
    /// Raised whenever a delegator's funds are still locked in but an attempt to undelegate was
    /// made.
    #[fail(display = "Delegator's funds are locked")]
    DelegatorFundsLocked = 38,

    // NOTE: These variants below and related plumbing will be removed once support for WASM
    // system contracts will be dropped.
    #[doc(hidden)]
    #[fail(display = "GasLimit")]
    GasLimit,

    #[cfg(test)]
    #[doc(hidden)]
    #[fail(display = "Sentinel error")]
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

    fn try_from(value: u8) -> result::Result<Self, Self::Error> {
        match value {
            d if d == Error::MissingKey as u8 => Ok(Error::MissingKey),
            d if d == Error::InvalidKeyVariant as u8 => Ok(Error::InvalidKeyVariant),
            d if d == Error::MissingValue as u8 => Ok(Error::MissingValue),
            d if d == Error::Serialization as u8 => Ok(Error::Serialization),
            d if d == Error::TransferToBidPurse as u8 => Ok(Error::TransferToBidPurse),
            d if d == Error::InvalidAmount as u8 => Ok(Error::InvalidAmount),
            d if d == Error::BidNotFound as u8 => Ok(Error::BidNotFound),
            d if d == Error::ValidatorNotFound as u8 => Ok(Error::ValidatorNotFound),
            d if d == Error::DelegatorNotFound as u8 => Ok(Error::DelegatorNotFound),
            d if d == Error::Storage as u8 => Ok(Error::Storage),
            d if d == Error::Bonding as u8 => Ok(Error::Bonding),
            d if d == Error::Unbonding as u8 => Ok(Error::Unbonding),
            d if d == Error::ReleaseFounderStake as u8 => Ok(Error::ReleaseFounderStake),
            d if d == Error::GetBalance as u8 => Ok(Error::GetBalance),
            d if d == Error::InvalidContext as u8 => Ok(Error::InvalidContext),
            d if d == Error::ValidatorFundsLocked as u8 => Ok(Error::ValidatorFundsLocked),
            d if d == Error::InvalidCaller as u8 => Ok(Error::InvalidCaller),
            d if d == Error::InvalidPublicKey as u8 => Ok(Error::InvalidPublicKey),
            d if d == Error::BondNotFound as u8 => Ok(Error::BondNotFound),
            d if d == Error::CreatePurseFailed as u8 => Ok(Error::CreatePurseFailed),
            d if d == Error::UnbondTooLarge as u8 => Ok(Error::UnbondTooLarge),
            d if d == Error::BondTooSmall as u8 => Ok(Error::BondTooSmall),
            d if d == Error::MissingDelegations as u8 => Ok(Error::MissingDelegations),
            d if d == Error::MismatchedEraValidators as u8 => Ok(Error::MismatchedEraValidators),
            d if d == Error::MintReward as u8 => Ok(Error::MintReward),
            d if d == Error::MintReduceTotalSupply as u8 => Ok(Error::MintReduceTotalSupply),
            d if d == Error::InvalidValidatorSlotsValue as u8 => {
                Ok(Error::InvalidValidatorSlotsValue)
            }
            d if d == Error::TransferToDelegatorPurse as u8 => Ok(Error::TransferToDelegatorPurse),
            d if d == Error::TransferToDelegatorPurse as u8 => Ok(Error::TransferToDelegatorPurse),
            d if d == Error::ValidatorRewardTransfer as u8 => Ok(Error::ValidatorRewardTransfer),
            d if d == Error::DelegatorRewardTransfer as u8 => Ok(Error::DelegatorRewardTransfer),
            d if d == Error::WithdrawDelegatorReward as u8 => Ok(Error::WithdrawDelegatorReward),
            d if d == Error::WithdrawValidatorReward as u8 => Ok(Error::WithdrawValidatorReward),
            d if d == Error::TransferToUnbondingPurse as u8 => Ok(Error::TransferToUnbondingPurse),

            d if d == Error::RecordEraInfo as u8 => Ok(Error::RecordEraInfo),
            d if d == Error::CLValue as u8 => Ok(Error::CLValue),
            d if d == Error::MissingSeigniorageRecipients as u8 => {
                Ok(Error::MissingSeigniorageRecipients)
            }
            d if d == Error::Transfer as u8 => Ok(Error::Transfer),
            d if d == Error::DelegationRateTooLarge as u8 => Ok(Error::DelegationRateTooLarge),
            d if d == Error::DelegatorFundsLocked as u8 => Ok(Error::DelegatorFundsLocked),
            d if d == Error::GasLimit as u8 => Ok(Error::GasLimit),
            _ => Err(TryFromU8ForError(())),
        }
    }
}

impl ToBytes for Error {
    fn to_bytes(&self) -> result::Result<Vec<u8>, bytesrepr::Error> {
        let value = *self as u8;
        value.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for Error {
    fn from_bytes(bytes: &[u8]) -> result::Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, rem): (u8, _) = FromBytes::from_bytes(bytes)?;
        let error: Error = value
            .try_into()
            // In case an Error variant is unable to be determined it would return an
            // Error::Formatting as if its unable to be correctly deserialized.
            .map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((error, rem))
    }
}

impl From<bytesrepr::Error> for Error {
    fn from(_: bytesrepr::Error) -> Self {
        Error::Serialization
    }
}

// This error type is not intended to be used by third party crates.
#[doc(hidden)]
pub enum PurseLookupError {
    KeyNotFound,
    KeyUnexpectedType,
}

impl From<PurseLookupError> for Error {
    fn from(error: PurseLookupError) -> Self {
        match error {
            PurseLookupError::KeyNotFound => Error::MissingKey,
            PurseLookupError::KeyUnexpectedType => Error::InvalidKeyVariant,
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
