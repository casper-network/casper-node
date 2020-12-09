use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    mint::{Mint, RuntimeProvider, StorageProvider, SystemProvider},
    system_contract_errors::mint::Error,
    CLTyped, CLValue, Key, URef, U512,
};

use super::Runtime;
use crate::{
    core::execution, shared::stored_value::StoredValue, storage::global_state::StateReader,
};

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }

    fn put_key(&mut self, name: &str, key: Key) {
        // TODO: update RuntimeProvider to better handle errors
        self.context
            .put_key(name.to_string(), key)
            .expect("should put key")
    }
    fn get_key(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }
}

// TODO: update Mint + StorageProvider to better handle errors
impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn new_uref<T: CLTyped + ToBytes>(&mut self, init: T) -> URef {
        let cl_value: CLValue = CLValue::from_t(init).expect("should convert value");
        self.context
            .new_uref(StoredValue::CLValue(cl_value))
            .expect("should create new uref")
    }

    fn write_local<K: ToBytes, V: CLTyped + ToBytes>(&mut self, key: K, value: V) {
        let key_bytes = key.to_bytes().expect("should serialize");
        let cl_value = CLValue::from_t(value).expect("should convert");
        self.context
            .write_ls(&key_bytes, cl_value)
            .expect("should write local state")
    }

    fn read_local<K: ToBytes, V: CLTyped + FromBytes>(
        &mut self,
        key: &K,
    ) -> Result<Option<V>, Error> {
        let key_bytes = key.to_bytes().expect("should serialize");
        let maybe_value = self
            .context
            .read_ls(&key_bytes)
            .map_err(|_| Error::Storage)?;
        match maybe_value {
            Some(value) => {
                let value = CLValue::into_t(value).unwrap();
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn read<T: CLTyped + FromBytes>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        let maybe_value = self
            .context
            .read_gs(&Key::URef(uref))
            .map_err(|_| Error::Storage)?;
        match maybe_value {
            Some(StoredValue::CLValue(value)) => {
                let value = CLValue::into_t(value).unwrap();
                Ok(Some(value))
            }
            Some(error) => panic!("should have received value: {:?}", error),
            None => Ok(None),
        }
    }

    fn write<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).expect("should convert");
        self.context
            .metered_write_gs(Key::URef(uref), StoredValue::CLValue(cl_value))
            .map_err(|_| Error::Storage)
    }

    fn add<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).expect("should convert");
        self.context
            .metered_add_gs(uref, cl_value)
            .map_err(|_| Error::Storage)
    }
}

impl<'a, R> SystemProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn record_transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error> {
        let result = Runtime::record_transfer(self, maybe_to, source, target, amount, id);
        result.map_err(|_| Error::RecordTransferFailure)
    }
}

impl<'a, R> Mint for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
