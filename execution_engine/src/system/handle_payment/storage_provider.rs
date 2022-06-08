use casper_types::{system::handle_payment::Error, URef, U512};

/// Provider of storage functionality.
pub trait StorageProvider {
    /// Write new balance.
    fn write_balance(&mut self, purse_uref: URef, amount: U512) -> Result<(), Error>;
}
