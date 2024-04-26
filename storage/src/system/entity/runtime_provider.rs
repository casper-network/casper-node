use casper_types::{system::entity::Error, AddressableEntity};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    fn entity_key(&self) -> Result<&AddressableEntity, Error>;
}
