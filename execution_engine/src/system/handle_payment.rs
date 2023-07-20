mod internal;
pub(crate) mod mint_provider;
pub(crate) mod runtime_provider;

use casper_types::{account::AccountHash, system::handle_payment::Error, AccessRights, URef, U512};

use crate::system::handle_payment::{
    mint_provider::MintProvider, runtime_provider::RuntimeProvider,
};

/// Handle payment functionality implementation.
pub trait HandlePayment: MintProvider + RuntimeProvider + Sized {
    /// Get payment purse.
    fn get_payment_purse(&self) -> Result<URef, Error> {
        let purse = internal::get_payment_purse(self)?;
        // Limit the access rights so only balance query and deposit are allowed.
        Ok(URef::new(purse.addr(), AccessRights::READ_ADD))
    }

    /// Set refund purse.
    fn set_refund_purse(&mut self, purse: URef) -> Result<(), Error> {
        internal::set_refund(self, purse)
    }

    /// Get refund purse.
    fn get_refund_purse(&self) -> Result<Option<URef>, Error> {
        // We purposely choose to remove the access rights so that we do not
        // accidentally give rights for a purse to some contract that is not
        // supposed to have it.
        let maybe_purse = internal::get_refund_purse(self)?;
        Ok(maybe_purse.map(|p| p.remove_access_rights()))
    }

    /// Finalize payment with `amount_spent` and a given `account`.
    fn finalize_payment(
        &mut self,
        amount_spent: U512,
        account: AccountHash,
        target: URef,
    ) -> Result<(), Error> {
        internal::finalize_payment(self, amount_spent, account, target)
    }
}
