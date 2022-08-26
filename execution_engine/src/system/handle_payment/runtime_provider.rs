use std::collections::BTreeSet;

use casper_types::{account::AccountHash, system::handle_payment::Error, BlockTime, Key, Phase};

use crate::core::engine_state::engine_config::{FeeHandling, RefundHandling};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// Get named key under a `name`.
    fn get_key(&self, name: &str) -> Option<Key>;

    /// Put key under a `name`.
    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error>;

    /// Remove a named key by `name`.
    fn remove_key(&mut self, name: &str) -> Result<(), Error>;

    /// Get current execution phase.
    fn get_phase(&self) -> Phase;

    /// Get current block time.
    fn get_block_time(&self) -> BlockTime;

    /// Get caller.
    fn get_caller(&self) -> AccountHash;

    /// Get refund handling.
    fn refund_handling(&self) -> &RefundHandling;

    /// Returns fee handling value.
    fn fee_handling(&self) -> FeeHandling;

    /// Returns list of administrative accounts.
    fn administrative_accounts(&self) -> &BTreeSet<AccountHash>;
}
