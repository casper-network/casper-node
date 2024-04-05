mod handle_payment_native;
mod internal;
pub mod mint_provider;
pub mod runtime_provider;
pub mod storage_provider;

use casper_types::{system::handle_payment::Error, AccessRights, HoldsEpoch, URef, U512};

use crate::system::handle_payment::{
    mint_provider::MintProvider, runtime_provider::RuntimeProvider,
    storage_provider::StorageProvider,
};

/// Handle payment functionality implementation.
pub trait HandlePayment: MintProvider + RuntimeProvider + StorageProvider + Sized {
    /// Get payment purse.
    fn get_payment_purse(&mut self) -> Result<URef, Error> {
        let purse = internal::get_payment_purse(self)?;
        // Limit the access rights so only balance query and deposit are allowed.
        Ok(URef::new(purse.addr(), AccessRights::READ_ADD))
    }

    /// Set refund purse.
    fn set_refund_purse(&mut self, purse: URef) -> Result<(), Error> {
        internal::set_refund(self, purse)
    }

    /// Get refund purse.
    fn get_refund_purse(&mut self) -> Result<Option<URef>, Error> {
        // We purposely choose to remove the access rights so that we do not
        // accidentally give rights for a purse to some contract that is not
        // supposed to have it.
        let maybe_purse = internal::get_refund_purse(self)?;
        Ok(maybe_purse.map(|p| p.remove_access_rights()))
    }

    /// Finalize payment with `amount_spent` and a given `account`.
    #[allow(clippy::too_many_arguments)]
    fn finalize_payment(
        &mut self,
        limit: U512,
        gas_price: u8,
        cost: U512,
        consumed: U512,
        source_purse: URef,
        target_purse: URef,
        holds_epoch: HoldsEpoch,
    ) -> Result<(), Error> {
        internal::finalize_payment(
            self,
            limit,
            gas_price,
            cost,
            consumed,
            source_purse,
            target_purse,
            holds_epoch,
        )
    }

    /// Distribute fees from an accumulation purse.
    fn distribute_accumulated_fees(
        &mut self,
        source_uref: URef,
        amount: Option<U512>,
    ) -> Result<(), Error> {
        internal::distribute_accumulated_fees(self, source_uref, amount)
    }

    fn burn(&mut self, source_uref: URef, amount: Option<U512>) -> Result<(), Error> {
        internal::burn(self, source_uref, amount)
    }
}
