//! Home of the Handle Payment contract's [`Error`] type.
use failure::Fail;

use alloc::vec::Vec;
use core::result;

use crate::{
    bytesrepr::{self, ToBytes, U8_SERIALIZED_LENGTH},
    CLType, CLTyped,
};

/// Errors which can occur while executing the Handle Payment contract.
// TODO: Split this up into user errors vs. system errors.
#[derive(Fail, Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    // ===== User errors =====
    /// The given validator is not bonded.
    #[fail(display = "Not bonded")]
    NotBonded = 0,
    /// There are too many bonding or unbonding attempts already enqueued to allow more.
    #[fail(display = "Too many events in queue")]
    TooManyEventsInQueue,
    /// At least one validator must remain bonded.
    #[fail(display = "Cannot unbond last validator")]
    CannotUnbondLastValidator,
    /// Failed to bond or unbond as this would have resulted in exceeding the maximum allowed
    /// difference between the largest and smallest stakes.
    #[fail(display = "Spread is too high")]
    SpreadTooHigh,
    /// The given validator already has a bond or unbond attempt enqueued.
    #[fail(display = "Multiple requests")]
    MultipleRequests,
    /// Attempted to bond with a stake which was too small.
    #[fail(display = "Bond is too small")]
    BondTooSmall,
    /// Attempted to bond with a stake which was too large.
    #[fail(display = "Bond is too large")]
    BondTooLarge,
    /// Attempted to unbond an amount which was too large.
    #[fail(display = "Unbond is too large")]
    UnbondTooLarge,
    /// While bonding, the transfer from source purse to the Handle Payment internal purse failed.
    #[fail(display = "Bond transfer failed")]
    BondTransferFailed,
    /// While unbonding, the transfer from the Handle Payment internal purse to the destination
    /// purse failed.
    #[fail(display = "Unbond transfer failed")]
    UnbondTransferFailed,
    // ===== System errors =====
    /// Internal error: a [`BlockTime`](crate::BlockTime) was unexpectedly out of sequence.
    #[fail(display = "Time went backwards")]
    TimeWentBackwards,
    /// Internal error: stakes were unexpectedly empty.
    #[fail(display = "Stakes not found")]
    StakesNotFound,
    /// Internal error: the Handle Payment contract's payment purse wasn't found.
    #[fail(display = "Payment purse not found")]
    PaymentPurseNotFound,
    /// Internal error: the Handle Payment contract's payment purse key was the wrong type.
    #[fail(display = "Payment purse has unexpected type")]
    PaymentPurseKeyUnexpectedType,
    /// Internal error: couldn't retrieve the balance for the Handle Payment contract's payment
    /// purse.
    #[fail(display = "Payment purse balance not found")]
    PaymentPurseBalanceNotFound,
    /// Internal error: the Handle Payment contract's bonding purse wasn't found.
    #[fail(display = "Bonding purse not found")]
    BondingPurseNotFound,
    /// Internal error: the Handle Payment contract's bonding purse key was the wrong type.
    #[fail(display = "Bonding purse key has unexpected type")]
    BondingPurseKeyUnexpectedType,
    /// Internal error: the Handle Payment contract's refund purse key was the wrong type.
    #[fail(display = "Refund purse key has unexpected type")]
    RefundPurseKeyUnexpectedType,
    /// Internal error: the Handle Payment contract's rewards purse wasn't found.
    #[fail(display = "Rewards purse not found")]
    RewardsPurseNotFound,
    /// Internal error: the Handle Payment contract's rewards purse key was the wrong type.
    #[fail(display = "Rewards purse has unexpected type")]
    RewardsPurseKeyUnexpectedType,
    // TODO: Put these in their own enum, and wrap them separately in `BondingError` and
    //       `UnbondingError`.
    /// Internal error: failed to deserialize the stake's key.
    #[fail(display = "Failed to deserialize stake's key")]
    StakesKeyDeserializationFailed,
    /// Internal error: failed to deserialize the stake's balance.
    #[fail(display = "Failed to deserialize stake's balance")]
    StakesDeserializationFailed,
    /// The invoked Handle Payment function can only be called by system contracts, but was called
    /// by a user contract.
    #[fail(display = "System function was called by user account")]
    SystemFunctionCalledByUserAccount,
    /// Internal error: while finalizing payment, the amount spent exceeded the amount available.
    #[fail(display = "Insufficient payment for amount spent")]
    InsufficientPaymentForAmountSpent,
    /// Internal error: while finalizing payment, failed to pay the validators (the transfer from
    /// the Handle Payment contract's payment purse to rewards purse failed).
    #[fail(display = "Transfer to rewards purse has failed")]
    FailedTransferToRewardsPurse,
    /// Internal error: while finalizing payment, failed to refund the caller's purse (the transfer
    /// from the Handle Payment contract's payment purse to refund purse or account's main purse
    /// failed).
    #[fail(display = "Transfer to account's purse failed")]
    FailedTransferToAccountPurse,
    /// Handle Payment contract's "set_refund_purse" method can only be called by the payment code
    /// of a deploy, but was called by the session code.
    #[fail(display = "Set refund purse was called outside payment")]
    SetRefundPurseCalledOutsidePayment,
    /// Raised when the system is unable to determine purse balance.
    #[fail(display = "Unable to get purse balance")]
    GetBalance,
    /// Raised when the system is unable to put named key.
    #[fail(display = "Unable to put named key")]
    PutKey,
    /// Raised when the system is unable to remove given named key.
    #[fail(display = "Unable to remove named key")]
    RemoveKey,
    /// Failed to transfer funds.
    #[fail(display = "Failed to transfer funds")]
    Transfer,
    // NOTE: These variants below will be removed once support for WASM system contracts will be
    // dropped.
    #[doc(hidden)]
    #[fail(display = "GasLimit")]
    GasLimit,
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
