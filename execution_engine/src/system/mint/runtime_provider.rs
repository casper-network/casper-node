use casper_types::{
    account::AccountHash,
    system::{mint::Error, CallStackElement},
    Key, Phase, StoredValue, URef, U512,
};

use crate::core::{engine_state::SystemContractRegistry, execution};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// This method should return the immediate caller of the current context.
    fn get_immediate_caller(&self) -> Option<&CallStackElement>;

    /// Get system contract registry.
    fn get_system_contract_registry(&self) -> Result<SystemContractRegistry, execution::Error>;

    fn is_called_from_standard_payment(&self) -> bool;

    fn read_account(
        &mut self,
        account_hash: &AccountHash,
    ) -> Result<Option<StoredValue>, execution::Error>;

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
    fn is_account_administrator(&self, account_hash: &AccountHash) -> Option<bool>;
    /// Checks if users can perform unrestricted transfers. This option is valid only for private
    /// chains.
    fn allow_unrestricted_transfers(&self) -> bool;
}
