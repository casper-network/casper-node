use casper_types::{
    account::AccountHash, system::entity::Error, AddressableEntity, Key, StoredValue,
};
use tracing::error;

use super::{runtime_provider::RuntimeProvider, storage_provider::StorageProvider, Entity};
use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::runtime_native::RuntimeNative,
    tracking_copy::TrackingCopyError,
};

impl<S> RuntimeProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller_new(&self) -> AccountHash {
        self.address()
    }
}

impl<S> StorageProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn read_key(&mut self, account: AccountHash) -> Result<Option<Key>, Error> {
        match self
            .tracking_copy()
            .borrow_mut()
            .read(&Key::Account(account))
        {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                Ok(Some(cl_value.into_t().map_err(|_| Error::CLValue)?))
            }
            Ok(Some(_)) => {
                error!("StorageProvider::read_key: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(TrackingCopyError::BytesRepr(_)) => Err(Error::Serialization),
            Err(err) => {
                error!("StorageProvider::read_key: {err:?}");
                Err(Error::Storage)
            }
        }
    }

    fn read_entity(&mut self, key: &Key) -> Result<Option<AddressableEntity>, Error> {
        match self.tracking_copy().borrow_mut().read(key) {
            Ok(Some(StoredValue::AddressableEntity(addressable_entity))) => {
                Ok(Some(addressable_entity))
            }
            Ok(Some(_)) => {
                error!("StorageProvider::read_key: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(TrackingCopyError::BytesRepr(_)) => Err(Error::Serialization),
            Err(err) => {
                error!("StorageProvider::read_key: {err:?}");
                Err(Error::Storage)
            }
        }
    }

    fn write_entity(&mut self, key: Key, entity: AddressableEntity) -> Result<(), Error> {
        // Charge for amount as measured by serialized length
        // let bytes_count = stored_value.serialized_length();
        // self.charge_gas_storage(bytes_count)?;

        self.tracking_copy()
            .borrow_mut()
            .write(key, StoredValue::AddressableEntity(entity));
        Ok(())
    }
}

impl<S> Entity for RuntimeNative<S> where S: StateReader<Key, StoredValue, Error = GlobalStateError> {}
