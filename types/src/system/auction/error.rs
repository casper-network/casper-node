//! Home of the Auction contract's [`enum@Error`] type.
use alloc::vec::Vec;
use core::{
    convert::{TryFrom, TryInto},
    result,
};

#[cfg(feature = "std")]
use thiserror::Error;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// Errors which can occur while executing the Auction contract.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(Error))]
#[cfg_attr(test, derive(strum::EnumIter))]
#[repr(u8)]
pub enum Error {
    /// Unable to find named key in the contract's named keys.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(0, Error::MissingKey as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Missing key"))]
    MissingKey = 0,
    /// Given named key contains invalid variant.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(1, Error::InvalidKeyVariant as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Invalid key variant"))]
    InvalidKeyVariant = 1,
    /// Value under an uref does not exist. This means the installer contract didn't work properly.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(2, Error::MissingValue as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Missing value"))]
    MissingValue = 2,
    /// ABI serialization issue while reading or writing.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(3, Error::Serialization as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Serialization error"))]
    Serialization = 3,
    /// Triggered when contract was unable to transfer desired amount of tokens into a bid purse.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(4, Error::TransferToBidPurse as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Transfer to bid purse error"))]
    TransferToBidPurse = 4,
    /// User passed invalid amount of tokens which might result in wrong values after calculation.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(5, Error::InvalidAmount as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Invalid amount"))]
    InvalidAmount = 5,
    /// Unable to find a bid by account hash in `active_bids` map.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(6, Error::BidNotFound as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Bid not found"))]
    BidNotFound = 6,
    /// Validator's account hash was not found in the map.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(7, Error::ValidatorNotFound as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Validator not found"))]
    ValidatorNotFound = 7,
    /// Delegator's account hash was not found in the map.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(8, Error::DelegatorNotFound as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Delegator not found"))]
    DelegatorNotFound = 8,
    /// Storage problem.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(9, Error::Storage as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Storage error"))]
    Storage = 9,
    /// Raised when system is unable to bond.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(10, Error::Bonding as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Bonding error"))]
    Bonding = 10,
    /// Raised when system is unable to unbond.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(11, Error::Unbonding as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Unbonding error"))]
    Unbonding = 11,
    /// Raised when Mint contract is unable to release founder stake.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(12, Error::ReleaseFounderStake as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Unable to release founder stake"))]
    ReleaseFounderStake = 12,
    /// Raised when the system is unable to determine purse balance.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(13, Error::GetBalance as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Unable to get purse balance"))]
    GetBalance = 13,
    /// Raised when an entry point is called from invalid account context.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(14, Error::InvalidContext as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Invalid context"))]
    InvalidContext = 14,
    /// Raised whenever a validator's funds are still locked in but an attempt to withdraw was
    /// made.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(15, Error::ValidatorFundsLocked as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Validator's funds are locked"))]
    ValidatorFundsLocked = 15,
    /// Raised when caller is not the system account.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(16, Error::InvalidCaller as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Function must be called by system account"))]
    InvalidCaller = 16,
    /// Raised when function is supplied a public key that does match the caller's or does not have
    /// an associated account.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(17, Error::InvalidPublicKey as u8);
    /// ```
    #[cfg_attr(
        feature = "std",
        error(
            "Supplied public key does not match caller's public key or has no associated account"
        )
    )]
    InvalidPublicKey = 17,
    /// Validator is not not bonded.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(18, Error::BondNotFound as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Validator's bond not found"))]
    BondNotFound = 18,
    /// Unable to create purse.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(19, Error::CreatePurseFailed as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Unable to create purse"))]
    CreatePurseFailed = 19,
    /// Attempted to unbond an amount which was too large.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(20, Error::UnbondTooLarge as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Unbond is too large"))]
    UnbondTooLarge = 20,
    /// Attempted to bond with a stake which was too small.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(21, Error::BondTooSmall as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Bond is too small"))]
    BondTooSmall = 21,
    /// Raised when rewards are to be distributed to delegators, but the validator has no
    /// delegations.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(22, Error::MissingDelegations as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Validators has not received any delegations"))]
    MissingDelegations = 22,
    /// The validators returned by the consensus component should match
    /// current era validators when distributing rewards.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(23, Error::MismatchedEraValidators as u8);
    /// ```
    #[cfg_attr(
        feature = "std",
        error("Mismatched era validator sets to distribute rewards")
    )]
    MismatchedEraValidators = 23,
    /// Failed to mint reward tokens.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(24, Error::MintReward as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Failed to mint rewards"))]
    MintReward = 24,
    /// Invalid number of validator slots.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(25, Error::InvalidValidatorSlotsValue as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Invalid number of validator slots"))]
    InvalidValidatorSlotsValue = 25,
    /// Failed to reduce total supply.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(26, Error::MintReduceTotalSupply as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Failed to reduce total supply"))]
    MintReduceTotalSupply = 26,
    /// Triggered when contract was unable to transfer desired amount of tokens into a delegators
    /// purse.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(27, Error::TransferToDelegatorPurse as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Transfer to delegators purse error"))]
    TransferToDelegatorPurse = 27,
    /// Triggered when contract was unable to perform a transfer to distribute validators reward.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(28, Error::ValidatorRewardTransfer as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Reward transfer to validator error"))]
    ValidatorRewardTransfer = 28,
    /// Triggered when contract was unable to perform a transfer to distribute delegators rewards.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(29, Error::DelegatorRewardTransfer as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Rewards transfer to delegator error"))]
    DelegatorRewardTransfer = 29,
    /// Failed to transfer desired amount while withdrawing delegators reward.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(30, Error::WithdrawDelegatorReward as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Withdraw delegator reward error"))]
    WithdrawDelegatorReward = 30,
    /// Failed to transfer desired amount while withdrawing validators reward.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(31, Error::WithdrawValidatorReward as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Withdraw validator reward error"))]
    WithdrawValidatorReward = 31,
    /// Failed to transfer desired amount into unbonding purse.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(32, Error::TransferToUnbondingPurse as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Transfer to unbonding purse error"))]
    TransferToUnbondingPurse = 32,
    /// Failed to record era info.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(33, Error::RecordEraInfo as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Record era info error"))]
    RecordEraInfo = 33,
    /// Failed to create a [`crate::CLValue`].
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(34, Error::CLValue as u8);
    /// ```
    #[cfg_attr(feature = "std", error("CLValue error"))]
    CLValue = 34,
    /// Missing seigniorage recipients for given era.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(35, Error::MissingSeigniorageRecipients as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Missing seigniorage recipients for given era"))]
    MissingSeigniorageRecipients = 35,
    /// Failed to transfer funds.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(36, Error::Transfer as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Transfer error"))]
    Transfer = 36,
    /// Delegation rate exceeds rate.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(37, Error::DelegationRateTooLarge as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Delegation rate too large"))]
    DelegationRateTooLarge = 37,
    /// Raised whenever a delegator's funds are still locked in but an attempt to undelegate was
    /// made.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(38, Error::DelegatorFundsLocked as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Delegator's funds are locked"))]
    DelegatorFundsLocked = 38,
    /// An arithmetic overflow has occurred.
    /// ```
    /// # use casper_types::system::auction::Error;
    /// assert_eq!(39, Error::ArithmeticOverflow as u8);
    /// ```
    #[cfg_attr(feature = "std", error("Arithmetic overflow"))]
    ArithmeticOverflow = 39,
    // NOTE: These variants below and related plumbing will be removed once support for WASM
    // system contracts will be dropped.
    #[doc(hidden)]
    #[cfg_attr(feature = "std", error("GasLimit"))]
    GasLimit,
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
            d if d == Error::ArithmeticOverflow as u8 => Ok(Error::ArithmeticOverflow),
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

    use strum::IntoEnumIterator;

    use super::Error;

    #[test]
    fn error_forward_trips() {
        for expected_error_variant in Error::iter() {
            assert_eq!(
                Error::try_from(expected_error_variant as u8),
                Ok(expected_error_variant)
            )
        }
    }

    #[test]
    fn error_backward_trips() {
        for u8 in 0..=u8::max_value() {
            match Error::try_from(u8) {
                Ok(error_variant) => {
                    assert_eq!(u8, error_variant as u8, "Error code mismatch")
                }
                Err(_) => continue,
            };
        }
    }
}
