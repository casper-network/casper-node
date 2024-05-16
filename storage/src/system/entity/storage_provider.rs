use casper_types::{account::AccountHash, system::entity::Error, AddressableEntity, Key};

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    fn read_key(&mut self, account: AccountHash) -> Result<Option<Key>, Error>;

    fn read_entity(&mut self, key: &Key) -> Result<Option<AddressableEntity>, Error>;

    fn write_entity(&mut self, key: Key, entity: AddressableEntity) -> Result<(), Error>;
}
