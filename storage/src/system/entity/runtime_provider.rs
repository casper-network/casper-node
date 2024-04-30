use casper_types::{account::AccountHash, system::entity::Error, AddressableEntity};

use crate::system::error::ProviderError;

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller_new(&self) -> AccountHash;

    fn entity_key(&self) -> Result<&AddressableEntity, Error>;
}
