use casper_types::{account::AccountHash, system::mint::Error, URef, U512};

/// Provides functionality of a system module.
pub trait SystemProvider {
    /// Records a transfer.
    fn record_transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error>;
}
