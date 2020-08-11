use crate::{account::AccountHash, TransferResult, URef, U512};

/// Provides an access to mint.
pub trait MintProvider {
    /// Transfer `amount` from `source` purse to a `target` account.
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> TransferResult;

    /// Transfer `amount` from `source` purse to a `target` purse.
    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ()>;

    /// Checks balance of a `purse`. Returns `None` if given purse does not exist.
    fn balance(&mut self, purse: URef) -> Option<U512>;

    fn read_base_round_reward(&mut self) -> U512 {
        todo!();
    }

    fn mint(&mut self, _amount: U512) -> URef {
        todo!();
    }
}
