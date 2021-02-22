//! Contains implementation of a standard payment contract implementation.
mod account_provider;
mod constants;
mod mint_provider;
mod proof_of_stake_provider;

use core::marker::Sized;

use crate::{ApiError, U512};

pub use crate::system::standard_payment::{
    account_provider::AccountProvider, constants::*, mint_provider::MintProvider,
    proof_of_stake_provider::ProofOfStakeProvider,
};

/// Implementation of a standard payment contract.
pub trait StandardPayment: AccountProvider + MintProvider + ProofOfStakeProvider + Sized {
    /// Pay `amount` to a payment purse.
    fn pay(&mut self, amount: U512) -> Result<(), ApiError> {
        let main_purse = self.get_main_purse()?;
        let payment_purse = self.get_payment_purse()?;
        self.transfer_purse_to_purse(main_purse, payment_purse, amount)
    }
}
