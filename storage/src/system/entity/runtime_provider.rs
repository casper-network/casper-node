use casper_types::{addressable_entity::AddKeyFailure, AddressableEntity};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    fn entity_key(&self) -> Result<&AddressableEntity, AddKeyFailure>;
}
