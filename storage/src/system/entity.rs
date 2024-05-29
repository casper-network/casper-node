mod entity_native;
pub mod runtime_provider;
pub mod storage_provider;

use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionType, Weight},
    system::entity::Error,
};

use self::{runtime_provider::RuntimeProvider, storage_provider::StorageProvider};

pub enum AssociatedKeyOp {
    Add,
    Update,
    Remove,
}

/// Entity trait.
pub trait Entity: RuntimeProvider + StorageProvider + Sized {
    /// Adds new associated key.
    fn add_associated_key(&mut self, account: AccountHash, weight: Weight) -> Result<(), Error> {
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

    /// Manage associated keys.
    fn manage_associated_keys(
        &mut self,
        key_list: Vec<(AccountHash, Weight, AssociatedKeyOp)>,
        key_mangement_threshold: Weight,
    ) -> Result<(), Error> {
        let caller = self.get_caller_new();
        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                // TODO:
                // addressable_entity.set_action_threshold_unchecked(ActionType::Deployment, deployment_threshold)?;
                // Maybe only this one:
                addressable_entity.set_action_threshold_unchecked(
                    ActionType::KeyManagement,
                    key_mangement_threshold,
                ).map_err(|_| Error::Threshold)?;
                // addressable_entity.set_action_threshold_unchecked(ActionType::UpgradeManagement, upgrade_mangement_threshold)?;

                // This should belong to `AddressableEntity`, but 3 different errors make it hard to implement.
                for (account, weight, op) in key_list {
                    match op {
                        AssociatedKeyOp::Add => {
                            addressable_entity.add_associated_key(account, weight)?
                        }
                        AssociatedKeyOp::Update => {
                            addressable_entity.update_associated_key_unchecked(account, weight)?
                        }
                        AssociatedKeyOp::Remove => {
                            addressable_entity.remove_associated_key_unchecked(account)?
                        }
                    }
                }
                addressable_entity.can_set_threshold(key_mangement_threshold).map_err(|_| Error::Threshold)?;
                // if addressable_entity.can_manage_keys_with(addressable_entity.associated_keys()) {
                //     self.write_entity(key, addressable_entity)?;
                // } else {
                //     return Err(Error::Threshold);
                // }
            }
        }
        Ok(())
    }
}
