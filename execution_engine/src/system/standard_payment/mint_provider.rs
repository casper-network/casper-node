use casper_types::{account::Account, ApiError, URef, U512};

/// Provides an access to mint.
pub trait MintProvider {
    /// Transfer `amount` of tokens from `source` purse to a `target` purse.
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target_account: &Account,
        amount: U512,
    ) -> Result<(), ApiError>;
}
