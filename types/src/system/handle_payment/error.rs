//! Home of the Handle Payment contract's [`enum@Error`] type.
use alloc::vec::Vec;
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
    result,
};

use crate::{
    bytesrepr::{self, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// Errors which can occur while executing the Handle Payment contract.
// TODO: Split this up into user errors vs. system errors.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    // ===== User errors =====
    /// The given validator is not bonded.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(0, Error::NotBonded as u8);
    /// ```
    NotBonded = 0,
    /// There are too many bonding or unbonding attempts already enqueued to allow more.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(1, Error::TooManyEventsInQueue as u8);
    /// ```
    TooManyEventsInQueue = 1,
    /// At least one validator must remain bonded.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(2, Error::CannotUnbondLastValidator as u8);
    /// ```
    CannotUnbondLastValidator = 2,
    /// Failed to bond or unbond as this would have resulted in exceeding the maximum allowed
    /// difference between the largest and smallest stakes.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(3, Error::SpreadTooHigh as u8);
    /// ```
    SpreadTooHigh = 3,
    /// The given validator already has a bond or unbond attempt enqueued.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(4, Error::MultipleRequests as u8);
    /// ```
    MultipleRequests = 4,
    /// Attempted to bond with a stake which was too small.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(5, Error::BondTooSmall as u8);
    /// ```
    BondTooSmall = 5,
    /// Attempted to bond with a stake which was too large.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(6, Error::BondTooLarge as u8);
    /// ```
    BondTooLarge = 6,
    /// Attempted to unbond an amount which was too large.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(7, Error::UnbondTooLarge as u8);
    /// ```
    UnbondTooLarge = 7,
    /// While bonding, the transfer from source purse to the Handle Payment internal purse failed.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(8, Error::BondTransferFailed as u8);
    /// ```
    BondTransferFailed = 8,
    /// While unbonding, the transfer from the Handle Payment internal purse to the destination
    /// purse failed.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(9, Error::UnbondTransferFailed as u8);
    /// ```
    UnbondTransferFailed = 9,
    // ===== System errors =====
    /// Internal error: a [`BlockTime`](crate::BlockTime) was unexpectedly out of sequence.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(10, Error::TimeWentBackwards as u8);
    /// ```
    TimeWentBackwards = 10,
    /// Internal error: stakes were unexpectedly empty.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(11, Error::StakesNotFound as u8);
    /// ```
    StakesNotFound = 11,
    /// Internal error: the Handle Payment contract's payment purse wasn't found.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(12, Error::PaymentPurseNotFound as u8);
    /// ```
    PaymentPurseNotFound = 12,
    /// Internal error: the Handle Payment contract's payment purse key was the wrong type.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(13, Error::PaymentPurseKeyUnexpectedType as u8);
    /// ```
    PaymentPurseKeyUnexpectedType = 13,
    /// Internal error: couldn't retrieve the balance for the Handle Payment contract's payment
    /// purse.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(14, Error::PaymentPurseBalanceNotFound as u8);
    /// ```
    PaymentPurseBalanceNotFound = 14,
    /// Internal error: the Handle Payment contract's bonding purse wasn't found.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(15, Error::BondingPurseNotFound as u8);
    /// ```
    BondingPurseNotFound = 15,
    /// Internal error: the Handle Payment contract's bonding purse key was the wrong type.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(16, Error::BondingPurseKeyUnexpectedType as u8);
    /// ```
    BondingPurseKeyUnexpectedType = 16,
    /// Internal error: the Handle Payment contract's refund purse key was the wrong type.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(17, Error::RefundPurseKeyUnexpectedType as u8);
    /// ```
    RefundPurseKeyUnexpectedType = 17,
    /// Internal error: the Handle Payment contract's rewards purse wasn't found.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(18, Error::RewardsPurseNotFound as u8);
    /// ```
    RewardsPurseNotFound = 18,
    /// Internal error: the Handle Payment contract's rewards purse key was the wrong type.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(19, Error::RewardsPurseKeyUnexpectedType as u8);
    /// ```
    RewardsPurseKeyUnexpectedType = 19,
    // TODO: Put these in their own enum, and wrap them separately in `BondingError` and
    //       `UnbondingError`.
    /// Internal error: failed to deserialize the stake's key.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(20, Error::StakesKeyDeserializationFailed as u8);
    /// ```
    StakesKeyDeserializationFailed = 20,
    /// Internal error: failed to deserialize the stake's balance.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(21, Error::StakesDeserializationFailed as u8);
    /// ```
    StakesDeserializationFailed = 21,
    /// The invoked Handle Payment function can only be called by system contracts, but was called
    /// by a user contract.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(22, Error::SystemFunctionCalledByUserAccount as u8);
    /// ```
    SystemFunctionCalledByUserAccount = 22,
    /// Internal error: while finalizing payment, the amount spent exceeded the amount available.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(23, Error::InsufficientPaymentForAmountSpent as u8);
    /// ```
    InsufficientPaymentForAmountSpent = 23,
    /// Internal error: while finalizing payment, failed to pay the validators (the transfer from
    /// the Handle Payment contract's payment purse to rewards purse failed).
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(24, Error::FailedTransferToRewardsPurse as u8);
    /// ```
    FailedTransferToRewardsPurse = 24,
    /// Internal error: while finalizing payment, failed to refund the caller's purse (the transfer
    /// from the Handle Payment contract's payment purse to refund purse or account's main purse
    /// failed).
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(25, Error::FailedTransferToAccountPurse as u8);
    /// ```
    FailedTransferToAccountPurse = 25,
    /// Handle Payment contract's "set_refund_purse" method can only be called by the payment code
    /// of a deploy, but was called by the session code.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(26, Error::SetRefundPurseCalledOutsidePayment as u8);
    /// ```
    SetRefundPurseCalledOutsidePayment = 26,
    /// Raised when the system is unable to determine purse balance.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(27, Error::GetBalance as u8);
    /// ```
    GetBalance = 27,
    /// Raised when the system is unable to put named key.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(28, Error::PutKey as u8);
    /// ```
    PutKey = 28,
    /// Raised when the system is unable to remove given named key.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(29, Error::RemoveKey as u8);
    /// ```
    RemoveKey = 29,
    /// Failed to transfer funds.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(30, Error::Transfer as u8);
    /// ```
    Transfer = 30,
    /// An arithmetic overflow occurred
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(31, Error::ArithmeticOverflow as u8);
    /// ```
    ArithmeticOverflow = 31,
    // NOTE: These variants below will be removed once support for WASM system contracts will be
    // dropped.
    #[doc(hidden)]
    GasLimit = 32,
    /// Refund purse is a payment purse.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(33, Error::RefundPurseIsPaymentPurse as u8);
    /// ```
    RefundPurseIsPaymentPurse = 33,
    /// Error raised while reducing total supply on the mint system contract.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(34, Error::ReduceTotalSupply as u8);
    /// ```
    ReduceTotalSupply = 34,
    /// Error writing to a storage.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(35, Error::Storage as u8);
    /// ```
    Storage = 35,
    /// Internal error: the Handle Payment contract's accumulation purse wasn't found.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(36, Error::AccumulationPurseNotFound as u8);
    /// ```
    AccumulationPurseNotFound = 36,
    /// Internal error: the Handle Payment contract's accumulation purse key was the wrong type.
    /// ```
    /// # use casper_types::system::handle_payment::Error;
    /// assert_eq!(37, Error::AccumulationPurseKeyUnexpectedType as u8);
    /// ```
    AccumulationPurseKeyUnexpectedType = 37,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::NotBonded => formatter.write_str("Not bonded"),
            Error::TooManyEventsInQueue => formatter.write_str("Too many events in queue"),
            Error::CannotUnbondLastValidator => formatter.write_str("Cannot unbond last validator"),
            Error::SpreadTooHigh => formatter.write_str("Spread is too high"),
            Error::MultipleRequests => formatter.write_str("Multiple requests"),
            Error::BondTooSmall => formatter.write_str("Bond is too small"),
            Error::BondTooLarge => formatter.write_str("Bond is too large"),
            Error::UnbondTooLarge => formatter.write_str("Unbond is too large"),
            Error::BondTransferFailed => formatter.write_str("Bond transfer failed"),
            Error::UnbondTransferFailed => formatter.write_str("Unbond transfer failed"),
            Error::TimeWentBackwards => formatter.write_str("Time went backwards"),
            Error::StakesNotFound => formatter.write_str("Stakes not found"),
            Error::PaymentPurseNotFound => formatter.write_str("Payment purse not found"),
            Error::PaymentPurseKeyUnexpectedType => {
                formatter.write_str("Payment purse has unexpected type")
            }
            Error::PaymentPurseBalanceNotFound => {
                formatter.write_str("Payment purse balance not found")
            }
            Error::BondingPurseNotFound => formatter.write_str("Bonding purse not found"),
            Error::BondingPurseKeyUnexpectedType => {
                formatter.write_str("Bonding purse key has unexpected type")
            }
            Error::RefundPurseKeyUnexpectedType => {
                formatter.write_str("Refund purse key has unexpected type")
            }
            Error::RewardsPurseNotFound => formatter.write_str("Rewards purse not found"),
            Error::RewardsPurseKeyUnexpectedType => {
                formatter.write_str("Rewards purse has unexpected type")
            }
            Error::StakesKeyDeserializationFailed => {
                formatter.write_str("Failed to deserialize stake's key")
            }
            Error::StakesDeserializationFailed => {
                formatter.write_str("Failed to deserialize stake's balance")
            }
            Error::SystemFunctionCalledByUserAccount => {
                formatter.write_str("System function was called by user account")
            }
            Error::InsufficientPaymentForAmountSpent => {
                formatter.write_str("Insufficient payment for amount spent")
            }
            Error::FailedTransferToRewardsPurse => {
                formatter.write_str("Transfer to rewards purse has failed")
            }
            Error::FailedTransferToAccountPurse => {
                formatter.write_str("Transfer to account's purse failed")
            }
            Error::SetRefundPurseCalledOutsidePayment => {
                formatter.write_str("Set refund purse was called outside payment")
            }
            Error::GetBalance => formatter.write_str("Unable to get purse balance"),
            Error::PutKey => formatter.write_str("Unable to put named key"),
            Error::RemoveKey => formatter.write_str("Unable to remove named key"),
            Error::Transfer => formatter.write_str("Failed to transfer funds"),
            Error::ArithmeticOverflow => formatter.write_str("Arithmetic overflow"),
            Error::GasLimit => formatter.write_str("GasLimit"),
            Error::RefundPurseIsPaymentPurse => {
                formatter.write_str("Refund purse is a payment purse.")
            }
            Error::ReduceTotalSupply => formatter.write_str("Failed to reduce total supply."),
            Error::Storage => formatter.write_str("Failed to write to storage."),
            Error::AccumulationPurseNotFound => formatter.write_str("Accumulation purse not found"),
            Error::AccumulationPurseKeyUnexpectedType => {
                formatter.write_str("Accumulation purse has unexpected type")
            }
        }
    }
}

impl TryFrom<u8> for Error {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let error = match value {
            v if v == Error::NotBonded as u8 => Error::NotBonded,
            v if v == Error::TooManyEventsInQueue as u8 => Error::TooManyEventsInQueue,
            v if v == Error::CannotUnbondLastValidator as u8 => Error::CannotUnbondLastValidator,
            v if v == Error::SpreadTooHigh as u8 => Error::SpreadTooHigh,
            v if v == Error::MultipleRequests as u8 => Error::MultipleRequests,
            v if v == Error::BondTooSmall as u8 => Error::BondTooSmall,
            v if v == Error::BondTooLarge as u8 => Error::BondTooLarge,
            v if v == Error::UnbondTooLarge as u8 => Error::UnbondTooLarge,
            v if v == Error::BondTransferFailed as u8 => Error::BondTransferFailed,
            v if v == Error::UnbondTransferFailed as u8 => Error::UnbondTransferFailed,
            v if v == Error::TimeWentBackwards as u8 => Error::TimeWentBackwards,
            v if v == Error::StakesNotFound as u8 => Error::StakesNotFound,
            v if v == Error::PaymentPurseNotFound as u8 => Error::PaymentPurseNotFound,
            v if v == Error::PaymentPurseKeyUnexpectedType as u8 => {
                Error::PaymentPurseKeyUnexpectedType
            }
            v if v == Error::PaymentPurseBalanceNotFound as u8 => {
                Error::PaymentPurseBalanceNotFound
            }
            v if v == Error::BondingPurseNotFound as u8 => Error::BondingPurseNotFound,
            v if v == Error::BondingPurseKeyUnexpectedType as u8 => {
                Error::BondingPurseKeyUnexpectedType
            }
            v if v == Error::RefundPurseKeyUnexpectedType as u8 => {
                Error::RefundPurseKeyUnexpectedType
            }
            v if v == Error::RewardsPurseNotFound as u8 => Error::RewardsPurseNotFound,
            v if v == Error::RewardsPurseKeyUnexpectedType as u8 => {
                Error::RewardsPurseKeyUnexpectedType
            }
            v if v == Error::StakesKeyDeserializationFailed as u8 => {
                Error::StakesKeyDeserializationFailed
            }
            v if v == Error::StakesDeserializationFailed as u8 => {
                Error::StakesDeserializationFailed
            }
            v if v == Error::SystemFunctionCalledByUserAccount as u8 => {
                Error::SystemFunctionCalledByUserAccount
            }
            v if v == Error::InsufficientPaymentForAmountSpent as u8 => {
                Error::InsufficientPaymentForAmountSpent
            }
            v if v == Error::FailedTransferToRewardsPurse as u8 => {
                Error::FailedTransferToRewardsPurse
            }
            v if v == Error::FailedTransferToAccountPurse as u8 => {
                Error::FailedTransferToAccountPurse
            }
            v if v == Error::SetRefundPurseCalledOutsidePayment as u8 => {
                Error::SetRefundPurseCalledOutsidePayment
            }

            v if v == Error::GetBalance as u8 => Error::GetBalance,
            v if v == Error::PutKey as u8 => Error::PutKey,
            v if v == Error::RemoveKey as u8 => Error::RemoveKey,
            v if v == Error::Transfer as u8 => Error::Transfer,
            v if v == Error::ArithmeticOverflow as u8 => Error::ArithmeticOverflow,
            v if v == Error::GasLimit as u8 => Error::GasLimit,
            v if v == Error::RefundPurseIsPaymentPurse as u8 => Error::RefundPurseIsPaymentPurse,
            v if v == Error::ReduceTotalSupply as u8 => Error::ReduceTotalSupply,
            v if v == Error::Storage as u8 => Error::Storage,
            v if v == Error::AccumulationPurseNotFound as u8 => Error::AccumulationPurseNotFound,
            v if v == Error::AccumulationPurseKeyUnexpectedType as u8 => {
                Error::AccumulationPurseKeyUnexpectedType
            }
            _ => return Err(()),
        };
        Ok(error)
    }
}

impl CLTyped for Error {
    fn cl_type() -> CLType {
        CLType::U8
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
