use casper_types::{
    addressable_entity::AddKeyFailure, system::auction::Error, AddressableEntity, Key, StoredValue,
};

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    fn read_key(&mut self, key: &Key) -> Result<Option<AddressableEntity>, Error>;

    fn write_key(&mut self, key: Key, value: StoredValue) -> Result<(), AddKeyFailure>;
}
