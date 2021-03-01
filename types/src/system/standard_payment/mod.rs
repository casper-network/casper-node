//! Contains implementation of a standard payment contract implementation.
mod account_provider;
mod constants;
mod handle_payment_provider;
mod mint_provider;

use core::marker::Sized;

use crate::{ApiError, U512};

pub use crate::system::standard_payment::{
    account_provider::AccountProvider, constants::*,
    handle_payment_provider::HandlePaymentProvider, mint_provider::MintProvider,
};

/// Implementation of a standard payment contract.
pub trait StandardPayment: AccountProvider + MintProvider + HandlePaymentProvider + Sized {
    /// Pay `amount` to a payment purse.
    fn pay(&mut self, amount: U512) -> Result<(), ApiError> {
        let main_purse = self.get_main_purse()?;
        let payment_purse = self.get_payment_purse()?;
        self.transfer_purse_to_purse(main_purse, payment_purse, amount)
    }
}
