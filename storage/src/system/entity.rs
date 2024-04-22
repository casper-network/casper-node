mod entity_native;
pub mod runtime_provider;
pub mod storage_provider;
// pub mod system_provider;

use casper_types::{
    account::AccountHash,
    addressable_entity::{AddKeyFailure, RemoveKeyFailure, UpdateKeyFailure, Weight},
    AddressableEntity,
};

use self::{runtime_provider::RuntimeProvider, storage_provider::StorageProvider};

/// Entity trait.
pub trait Entity: RuntimeProvider + StorageProvider + Sized {
    /// Adds new associated key.
    fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), AddKeyFailure> {
        let entity_key = self.entity_key();

        // Get the current entity record
        let addressable_entity = {
            let mut entity = self.read_key(&entity_key)?;
            // enforce max keys limit
            if entity.associated_keys().len() >= self.engine_config.max_associated_keys() as usize {
                return Err(AddKeyFailure::MaxKeysLimit);
            }

            // Exit early in case of error without updating global state
            entity.add_associated_key(account_hash, weight)?;
            entity
        };
        let entity = addressable_entity;

        self.write_key(
            entity_key,
            self.addressable_entity_to_validated_value(entity)?,
        )?;

        Ok(())
    }

    /// Remove associated key.
    fn remove_associated_key(&mut self, account_hash: AccountHash) -> Result<(), RemoveKeyFailure> {
        let entity_key = self.entity_key();

        if !self
            .addressable_entity
            .can_manage_keys_with(&self.authorization_keys)
        {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(RemoveKeyFailure::PermissionDenied);
        }

        // Converts an account's public key into a URef
        let contract_hash = self.get_entity_key();

        // Take an account out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&contract_hash)?;

        // Exit early in case of error without updating global state
        entity.remove_associated_key(account_hash)?;

        let account_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(contract_hash, account_value)?;

        Ok(())
    }

    /// Update associated key.
    fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), UpdateKeyFailure> {
        let entity_key = self.entity_key();

        if !self.entity().can_manage_keys_with(&self.authorization_keys) {
            // Exit early if authorization keys weight doesn't exceed required
            // key management threshold
            return Err(UpdateKeyFailure::PermissionDenied);
        }

        // Converts an account's public key into a URef
        let key = self.get_entity_key();

        // Take an account out of the global state
        let mut entity: AddressableEntity = self.read_gs_typed(&key)?;

        // Exit early in case of error without updating global state
        entity.update_associated_key(account_hash, weight)?;

        let entity_value = self.addressable_entity_to_validated_value(entity)?;

        self.metered_write_gs_unsafe(key, entity_value)?;

        Ok(())
    }
}
