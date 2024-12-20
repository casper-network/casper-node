mod handle_payment_native;
mod internal;
/// Provides mint logic for handle payment processing.
pub mod mint_provider;
/// Provides runtime logic for handle payment processing.
pub mod runtime_provider;
/// Provides storage logic for handle payment processing.
pub mod storage_provider;

use casper_types::{
    system::handle_payment::{Error, REFUND_PURSE_KEY},
    AccessRights, PublicKey, URef, U512,
};
use num_rational::Ratio;
use tracing::error;

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
        // make sure the passed uref is actually a purse...
        // if it has a balance it is a purse and if not it isn't
        let _balance = self.available_balance(purse)?;
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

    /// Clear refund purse.
    fn clear_refund_purse(&mut self) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            error!("invalid caller to clear refund purse");
            return Err(Error::InvalidCaller);
        }

        self.remove_key(REFUND_PURSE_KEY)
    }

    /// Calculate overpayment and fees (if any) for payment finalization.
    #[allow(clippy::too_many_arguments)]
    fn calculate_overpayment_and_fee(
        &mut self,
        limit: U512,
        gas_price: u8,
        cost: U512,
        consumed: U512,
        source_purse: URef,
        refund_ratio: Ratio<U512>,
    ) -> Result<(U512, U512), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            error!("invalid caller to calculate overpayment and fee");
            return Err(Error::InvalidCaller);
        }

        let available_balance = match self.available_balance(source_purse)? {
            Some(balance) => balance,
            None => return Err(Error::PaymentPurseBalanceNotFound),
        };
        internal::calculate_overpayment_and_fee(
            limit,
            gas_price,
            cost,
            consumed,
            available_balance,
            refund_ratio,
        )
    }

    /// Distribute fees from an accumulation purse.
    fn distribute_accumulated_fees(
        &mut self,
        source_uref: URef,
        amount: Option<U512>,
    ) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            error!("invalid caller to distribute accumulated fee");
            return Err(Error::InvalidCaller);
        }

        internal::distribute_accumulated_fees(self, source_uref, amount)
    }

    /// Burns the imputed amount from the imputed purse.
    fn payment_burn(&mut self, source_uref: URef, amount: Option<U512>) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            error!("invalid caller to payment burn");
            return Err(Error::InvalidCaller);
        }

        internal::payment_burn(self, source_uref, amount)
    }
}
