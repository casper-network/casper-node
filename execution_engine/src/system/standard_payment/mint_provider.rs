use casper_types::{ApiError, URef, U512};

/// Provides an access to mint.
pub trait MintProvider {
    /// Transfer `amount` of tokens from `source` purse to a `target` purse.
    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ApiError>;
}
