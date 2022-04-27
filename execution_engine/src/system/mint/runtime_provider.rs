use casper_types::{
    account::AccountHash,
    system::{mint::Error, CallStackElement},
    Key, Phase, URef, U512,
};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// This method should return the immediate caller of the current context.
    fn get_immediate_caller(&self) -> Option<&CallStackElement>;

    /// Gets execution phase
    fn get_phase(&self) -> Phase;

    /// This method should handle storing given [`Key`] under `name`.
    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error>;

    /// This method should handle obtaining a given named [`Key`] under a `name`.
    fn get_key(&self, name: &str) -> Option<Key>;

    /// Returns approved CSPR spending limit.
    fn get_approved_spending_limit(&self) -> U512;

    /// Signal to host that `transferred` amount of tokens has been transferred.
    fn sub_approved_spending_limit(&mut self, transferred: U512);

    /// Returns main purse of the sender account.
    fn get_main_purse(&self) -> URef;

    /// Returns Some(_) with a flag indicating if execution is started by an admin account. This is
    /// valid only for private chains.
    ///
    /// On public chains this will always return None.
    fn is_admin(&mut self, account_hash: &AccountHash) -> Option<bool>;
    /// Checks if chain is operating as a private one (true), or public (false).
    fn is_private_chain(&mut self) -> bool;
    /// Checks if we're currently executing a host function.
    fn is_in_host_function(&mut self) -> bool;
}
