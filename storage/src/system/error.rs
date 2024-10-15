use casper_types::account::AccountHash;

/// Implementation level errors for system contract providers
#[derive(Debug)]
pub enum ProviderError {
    /// System contract registry.
    SystemEntityRegistry,
    /// Account hash.
    AccountHash(AccountHash),
}
