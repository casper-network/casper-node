use casper_types::{addressable_entity::AddKeyFailure, AddressableEntity, Key, StoredValue};

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    fn read_key(&mut self, key: &Key) -> Result<Option<AddressableEntity>, AddKeyFailure>;

    fn write_key(&mut self, key: Key, value: StoredValue) -> Result<(), AddKeyFailure>;
}
