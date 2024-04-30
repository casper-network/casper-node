use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::entity::{
        runtime_provider::RuntimeProvider, storage_provider::StorageProvider, Entity,
    },
};
use casper_types::{
    account::AccountHash, addressable_entity::Weight, system::entity::Error, AddressableEntity,
    Key, StoredValue,
};
use tracing::error;

use super::Runtime;
use crate::execution::ExecError;

impl From<ExecError> for Option<Error> {
    fn from(exec_error: ExecError) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure [`Auction`]
            // contract running natively supports propagating gas limit errors without a panic.
            ExecError::GasLimit => Some(Error::GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller_new(&self) -> AccountHash {
        self.context.get_caller()
    }

    fn entity_key(&self) -> Result<&AddressableEntity, Error> {
        unimplemented!()
    }
}

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn read_key(&mut self, account_hash: AccountHash) -> Result<Option<Key>, Error> {
        match self.context.read_gs(&Key::Account(account_hash)) {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                Ok(Some(cl_value.into_t().map_err(|_| Error::CLValue)?))
            }
            Ok(Some(_)) => {
                error!("StorageProvider::read: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(ExecError::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(ExecError::GasLimit) => Err(Error::GasLimit),
            Err(err) => {
                error!("StorageProvider::read: {err:?}");
                Err(Error::Storage)
            }
        }
    }

    fn read_entity(&mut self, key: &Key) -> Result<Option<AddressableEntity>, Error> {
        match self.context.read_gs(key) {
            Ok(Some(StoredValue::AddressableEntity(entity))) => Ok(Some(entity)),
            Ok(Some(_)) => {
                error!("StorageProvider::read_key: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(ExecError::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(ExecError::GasLimit) => Err(Error::GasLimit),
            Err(err) => {
                error!("StorageProvider::read_key: {err:?}");
                Err(Error::Storage)
            }
        }
    }

    fn write_entity(&mut self, key: Key, entity: AddressableEntity) -> Result<(), Error> {
        self.context
            .metered_write_gs_unsafe(key, StoredValue::AddressableEntity(entity))
            .map_err(|exec_error| {
                error!("StorageProvider::write_entity: {exec_error:?}");
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })
    }
}

impl<'a, R> Entity for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn add_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), Error> {
        let caller = self.get_caller_new();
        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                addressable_entity.add_associated_key(account_hash, weight)?;
                self.write_entity(key, addressable_entity)?;
            }
        }
        Ok(())
    }

    fn remove_associated_key(&mut self, account_hash: AccountHash) -> Result<(), Error> {
        let caller = self.get_caller_new();
        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                addressable_entity.remove_associated_key(account_hash)?;
                self.write_entity(key, addressable_entity)?;
            }
        }
        Ok(())
    }

    fn update_associated_key(
        &mut self,
        account_hash: AccountHash,
        weight: Weight,
    ) -> Result<(), Error> {
        let caller = self.get_caller_new();
        if let Some(key) = self.read_key(caller)? {
            if let Some(mut addressable_entity) = self.read_entity(&key)? {
                addressable_entity.update_associated_key(account_hash, weight)?;
                self.write_entity(key, addressable_entity)?;
            }
        }
        Ok(())
    }
}
