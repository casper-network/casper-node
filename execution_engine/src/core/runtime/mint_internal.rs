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

impl From<execution::Error> for Option<Error> {
    fn from(exec_error: execution::Error) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure [`Mint`]
            // contract running natively supports propagating gas limit errors without a panic.
            execution::Error::GasLimit => Some(Error::GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
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
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::PutKey))
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
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::NewURef))
    }

    fn write_balance_entry(&mut self, purse_key: URef, balance_uref: URef) -> Result<(), Error> {
        let cl_value = CLValue::from_t(Key::URef(balance_uref)).map_err(|_| Error::CLValue)?;
        self.context
            .write_purse_uref(purse_key, cl_value)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::WriteLocal))
    }

    fn read_balance_entry(&mut self, purse_key: &URef) -> Result<Option<Key>, Error> {
        let maybe_value = self
            .context
            .read_purse_uref(purse_key)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))?;
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
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))?;
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
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn add<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .metered_add_gs(uref, cl_value)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
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
        result.map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::RecordTransferFailure)
        })
    }
}

impl<'a, R> Mint for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
