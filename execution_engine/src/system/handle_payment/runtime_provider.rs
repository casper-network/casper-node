use crate::core::engine_state::engine_config::{FeeHandling, RefundHandling};
use casper_types::{account::AccountHash, system::handle_payment::Error, BlockTime, Key, Phase};

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

    /// Returns Some(_) with a flag indicating if execution is started by an admin account. This is
    /// valid only for private chains.
    ///
    /// On public chains this will always return None.
    fn is_account_administrator(&self, account_hash: &AccountHash) -> Option<bool>;

    /// Returns fee handling value.
    fn fee_handling(&self) -> FeeHandling;
}
