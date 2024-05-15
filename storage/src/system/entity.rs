mod entity_native;
pub mod runtime_provider;
pub mod storage_provider;

use casper_types::{account::AccountHash, addressable_entity::Weight, system::entity::Error};

use self::{runtime_provider::RuntimeProvider, storage_provider::StorageProvider};

/// Entity trait.
pub trait Entity: RuntimeProvider + StorageProvider + Sized {
    /// Adds new associated key.
    fn add_associated_key(&mut self, account: AccountHash, weight: Weight) -> Result<(), Error> {
        // let entity_key = self.entity_key().unwrap(); // FIXME: unwrap()
        let caller = self.get_caller_new();

        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                addressable_entity.add_associated_key(account, weight)?;
                self.write_entity(key, addressable_entity)?;
            }
        }

        Ok(())
    }

    /// Remove associated key.
    fn remove_associated_key(&mut self, account: AccountHash) -> Result<(), Error> {
        let caller = self.get_caller_new();
        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                addressable_entity.remove_associated_key(account)?;
                self.write_entity(key, addressable_entity)?;
            }
        }
        Ok(())
    }

    /// Update associated key.
    fn update_associated_key(&mut self, account: AccountHash, weight: Weight) -> Result<(), Error> {
        let caller = self.get_caller_new();
        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                addressable_entity.update_associated_key(account, weight)?;
                self.write_entity(key, addressable_entity)?;
            }
        }
        Ok(())
    }
}
