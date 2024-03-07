use crate::system::error::ProviderError;
use casper_types::{
    account::AccountHash, system::Caller, AddressableEntity, Key, Phase, SystemEntityRegistry,
    URef, U512,
};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// This method should return the immediate caller of the current context.
    fn get_immediate_caller(&self) -> Option<Caller>;

    fn is_called_from_standard_payment(&self) -> bool;

    /// Get system contract registry.
    fn get_system_entity_registry(&self) -> Result<SystemEntityRegistry, ProviderError>;

    fn read_addressable_entity_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Option<AddressableEntity>, ProviderError>;

    /// Gets execution phase
    fn get_phase(&self) -> Phase;

    /// This method should handle obtaining a given named [`Key`] under a `name`.
    fn get_key(&self, name: &str) -> Option<Key>;

    /// Returns approved CSPR spending limit.
    fn get_approved_spending_limit(&self) -> U512;

    /// Signal to host that `amount` of tokens has been transferred.
    fn sub_approved_spending_limit(&mut self, amount: U512);

    /// Returns main purse of the sender account.
    fn get_main_purse(&self) -> URef;

    /// Returns `true` if the account hash belongs to an administrator account, otherwise `false`.
    fn is_administrator(&self, account_hash: &AccountHash) -> bool;

    /// Checks if users can perform unrestricted transfers. This option is valid only for private
    /// chains.
    fn allow_unrestricted_transfers(&self) -> bool;
}
