use casper_types::{
    account::AccountHash, system::handle_payment::Error, TransferredTo, URef, U512,
};

/// Provides an access to mint.
pub trait MintProvider {
    /// Transfer `amount` from `source` purse to a `target` account.
    /// Note: the source should always be a system purse of some kind,
    /// such as the payment purse or an accumulator purse.
    /// The target should be the recipient of a refund or a reward
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, Error>;

    /// Transfer `amount` from `source` purse to a `target` purse.
    /// Note: the source should always be a system purse of some kind,
    /// such as the payment purse or an accumulator purse.
    /// The target should be the recipient of a refund or a reward
    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error>;

    /// Checks balance of a `purse`. Returns `None` if given purse does not exist.
    fn available_balance(&mut self, purse: URef) -> Result<Option<U512>, Error>;

    /// Reduce total supply by `amount`.
    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error>;
}
