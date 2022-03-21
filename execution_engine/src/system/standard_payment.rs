pub(crate) mod account_provider;
pub(crate) mod handle_payment_provider;
pub(crate) mod mint_provider;

use casper_types::{ApiError, U512};

use self::{
    account_provider::AccountProvider, handle_payment_provider::HandlePaymentProvider,
    mint_provider::MintProvider,
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
