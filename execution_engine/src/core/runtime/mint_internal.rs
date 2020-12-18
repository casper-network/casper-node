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

/// Translates [`execution::Error`] into mint-specific [`Error`].
///
/// This function is primarily used to propagate [`Error::GasLimit`] to make sure [`Mint`]
/// contract running natively supports propagating gas limit errors without a panic.
fn to_mint_error(exec_error: execution::Error, unhandled: Error) -> Error {
    match exec_error {
        execution::Error::GasLimit => Error::GasLimit,
        execution::Error::InvalidContext => Error::InvalidContext,
        // There are possibly other exec errors happening but such transalation would be lossy.
        _ => unhandled,
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }

    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error> {
        self.context
            .put_key(name.to_string(), key)
            .map_err(|e| to_mint_error(e, Error::PutKey))
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
    fn new_uref<T: CLTyped + ToBytes>(&mut self, init: T) -> Result<URef, Error> {
        let cl_value: CLValue = CLValue::from_t(init).map_err(|_| Error::CLValue)?;
        self.context
            .new_uref(StoredValue::CLValue(cl_value))
            .map_err(|e| to_mint_error(e, Error::NewURef))
    }

    fn write_local<K: ToBytes, V: CLTyped + ToBytes>(
        &mut self,
        key: K,
        value: V,
    ) -> Result<(), Error> {
        let key_bytes = key.to_bytes().map_err(|_| Error::Serialize)?;
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .write_ls(&key_bytes, cl_value)
            .map_err(|e| to_mint_error(e, Error::WriteLocal))
    }

    fn read_local<K: ToBytes, V: CLTyped + FromBytes>(
        &mut self,
        key: &K,
    ) -> Result<Option<V>, Error> {
        let key_bytes = key.to_bytes().map_err(|_| Error::Serialize)?;
        let maybe_value = self
            .context
            .read_ls(&key_bytes)
            .map_err(|e| to_mint_error(e, Error::Storage))?;
        match maybe_value {
            Some(value) => {
                let value = CLValue::into_t(value).map_err(|_| Error::CLValue)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn read<T: CLTyped + FromBytes>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        let maybe_value = self
            .context
            .read_gs(&Key::URef(uref))
            .map_err(|e| to_mint_error(e, Error::Storage))?;
        match maybe_value {
            Some(StoredValue::CLValue(value)) => {
                let value = CLValue::into_t(value).map_err(|_| Error::CLValue)?;
                Ok(Some(value))
            }
            Some(_cl_value) => Err(Error::CLValue),
            None => Ok(None),
        }
    }

    fn write<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .metered_write_gs(Key::URef(uref), StoredValue::CLValue(cl_value))
            .map_err(|e| to_mint_error(e, Error::Storage))
    }

    fn add<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .metered_add_gs(uref, cl_value)
            .map_err(|e| to_mint_error(e, Error::Storage))
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
        result.map_err(|e| to_mint_error(e, Error::RecordTransferFailure))
    }
}

impl<'a, R> Mint for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
