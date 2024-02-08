use casper_types::{URef, U512};

use crate::system::handle_payment::Error;

/// Provider of storage functionality.
pub trait StorageProvider {
    /// Write new balance.
    fn write_balance(&mut self, purse_uref: URef, amount: U512) -> Result<(), Error>;
}
