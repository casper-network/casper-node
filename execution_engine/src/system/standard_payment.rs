pub(crate) mod account_provider;
pub(crate) mod handle_payment_provider;
pub(crate) mod mint_provider;

use casper_types::{account::Account, ApiError, PublicKey, U512};

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

        // NOTE: We can't simply pass a virtual system account with main purse assumed to be a
        // payment purse - as some chains may have started with a system account with a main purse
        // of different purpose.
        let virtual_system_account = Account::new(
            PublicKey::System.to_account_hash(),
            Default::default(),
            payment_purse,
            Default::default(),
            Default::default(),
        );

        self.transfer_purse_to_account(main_purse, &virtual_system_account, amount)
    }
}
