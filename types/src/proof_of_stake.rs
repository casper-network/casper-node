//! Contains implementation of a Proof Of Stake contract functionality.
mod constants;
mod mint_provider;
mod runtime_provider;

use core::marker::Sized;

use crate::{account::AccountHash, system_contract_errors::pos::Result, AccessRights, URef, U512};

pub use crate::proof_of_stake::{
    constants::*, mint_provider::MintProvider, runtime_provider::RuntimeProvider,
};

/// Proof of stake functionality implementation.
pub trait ProofOfStake: MintProvider + RuntimeProvider + Sized {
    /// Get payment purse.
    fn get_payment_purse(&self) -> Result<URef> {
        let purse = internal::get_payment_purse(self)?;
        // Limit the access rights so only balance query and deposit are allowed.
        Ok(URef::new(purse.addr(), AccessRights::READ_ADD))
    }

    /// Set refund purse.
    fn set_refund_purse(&mut self, purse: URef) -> Result<()> {
        internal::set_refund(self, purse)
    }

    /// Get refund purse.
    fn get_refund_purse(&self) -> Result<Option<URef>> {
        // We purposely choose to remove the access rights so that we do not
        // accidentally give rights for a purse to some contract that is not
        // supposed to have it.
        let maybe_purse = internal::get_refund_purse(self)?;
        Ok(maybe_purse.map(|p| p.remove_access_rights()))
    }

    /// Finalize payment with `amount_spent` and a given `account`.
    fn finalize_payment(&mut self, amount_spent: U512, account: AccountHash) -> Result<()> {
        internal::finalize_payment(self, amount_spent, account)
    }
}

mod internal {
    use crate::{
        account::AccountHash,
        system_contract_errors::pos::{Error, PurseLookupError, Result},
        Key, Phase, URef, U512,
    };

    use crate::proof_of_stake::{mint_provider::MintProvider, runtime_provider::RuntimeProvider};

    /// Account used to run system functions (in particular `finalize_payment`).
    const SYSTEM_ACCOUNT: AccountHash = AccountHash::new([0u8; 32]);

    /// The uref name where the PoS accepts payment for computation on behalf of validators.
    const PAYMENT_PURSE_KEY: &str = "pos_payment_purse";

    /// The uref name where the PoS holds validator earnings before distributing them.
    const REWARDS_PURSE_KEY: &str = "pos_rewards_purse";

    /// The uref name where the PoS will refund unused payment back to the user. The uref this name
    /// corresponds to is set by the user.
    const REFUND_PURSE_KEY: &str = "pos_refund_purse";

    /// Attempts to look up a purse from the named_keys
    fn get_purse<R: RuntimeProvider>(
        runtime_provider: &R,
        name: &str,
    ) -> core::result::Result<URef, PurseLookupError> {
        runtime_provider
            .get_key(name)
            .ok_or(PurseLookupError::KeyNotFound)
            .and_then(|key| match key {
                Key::URef(uref) => Ok(uref),
                _ => Err(PurseLookupError::KeyUnexpectedType),
            })
    }

    /// Returns the purse for accepting payment for transactions.
    pub fn get_payment_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef> {
        get_purse::<R>(runtime_provider, PAYMENT_PURSE_KEY).map_err(PurseLookupError::payment)
    }

    /// Returns the purse for holding validator earnings
    pub fn get_rewards_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<URef> {
        get_purse::<R>(runtime_provider, REWARDS_PURSE_KEY).map_err(PurseLookupError::rewards)
    }

    /// Sets the purse where refunds (excess funds not spent to pay for computation) will be sent.
    /// Note that if this function is never called, the default location is the main purse of the
    /// deployer's account.
    pub fn set_refund<R: RuntimeProvider>(runtime_provider: &mut R, purse: URef) -> Result<()> {
        if let Phase::Payment = runtime_provider.get_phase() {
            runtime_provider.put_key(REFUND_PURSE_KEY, Key::URef(purse));
            return Ok(());
        }
        Err(Error::SetRefundPurseCalledOutsidePayment)
    }

    /// Returns the currently set refund purse.
    pub fn get_refund_purse<R: RuntimeProvider>(runtime_provider: &R) -> Result<Option<URef>> {
        match get_purse::<R>(runtime_provider, REFUND_PURSE_KEY) {
            Ok(uref) => Ok(Some(uref)),
            Err(PurseLookupError::KeyNotFound) => Ok(None),
            Err(PurseLookupError::KeyUnexpectedType) => Err(Error::RefundPurseKeyUnexpectedType),
        }
    }

    /// Transfers funds from the payment purse to the validator rewards purse, as well as to the
    /// refund purse, depending on how much was spent on the computation. This function maintains
    /// the invariant that the balance of the payment purse is zero at the beginning and end of each
    /// deploy and that the refund purse is unset at the beginning and end of each deploy.
    pub fn finalize_payment<P: MintProvider + RuntimeProvider>(
        provider: &mut P,
        amount_spent: U512,
        account: AccountHash,
    ) -> Result<()> {
        let caller = provider.get_caller();
        if caller != SYSTEM_ACCOUNT {
            return Err(Error::SystemFunctionCalledByUserAccount);
        }

        let payment_purse = get_payment_purse(provider)?;
        let total = match provider.balance(payment_purse) {
            Some(balance) => balance,
            None => return Err(Error::PaymentPurseBalanceNotFound),
        };

        if total < amount_spent {
            return Err(Error::InsufficientPaymentForAmountSpent);
        }
        let refund_amount = total - amount_spent;

        let rewards_purse = get_rewards_purse(provider)?;
        let refund_purse = get_refund_purse(provider)?;
        provider.remove_key(REFUND_PURSE_KEY); //unset refund purse after reading it

        // pay validators
        provider
            .transfer_purse_to_purse(payment_purse, rewards_purse, amount_spent)
            .map_err(|_| Error::FailedTransferToRewardsPurse)?;

        if refund_amount.is_zero() {
            return Ok(());
        }

        // give refund
        let refund_purse = match refund_purse {
            Some(uref) => uref,
            None => return refund_to_account::<P>(provider, payment_purse, account, refund_amount),
        };

        // in case of failure to transfer to refund purse we fall back on the account's main purse
        if provider
            .transfer_purse_to_purse(payment_purse, refund_purse, refund_amount)
            .is_err()
        {
            return refund_to_account::<P>(provider, payment_purse, account, refund_amount);
        }

        Ok(())
    }

    pub fn refund_to_account<M: MintProvider>(
        mint_provider: &mut M,
        payment_purse: URef,
        account: AccountHash,
        amount: U512,
    ) -> Result<()> {
        match mint_provider.transfer_purse_to_account(payment_purse, account, amount) {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::FailedTransferToAccountPurse),
        }
    }
}
