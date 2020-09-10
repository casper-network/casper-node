use casper_types::{
    auction::{AuctionProvider, RuntimeProvider, StorageProvider, SystemProvider},
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::Error,
    CLTyped, CLValue, Key, URef, U512,
};

use super::Runtime;
use crate::{
    core::execution, shared::stored_value::StoredValue, storage::global_state::StateReader,
};

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_key(&mut self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        match self.context.read_gs(&uref.into()) {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                Ok(Some(cl_value.into_t().map_err(|_| Error::Storage)?))
            }
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(None),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).unwrap();
        self.context
            .write_gs(uref.into(), StoredValue::CLValue(cl_value))
            .map_err(|_| Error::Storage)
    }
}

impl<'a, R> SystemProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn create_purse(&mut self) -> URef {
        Runtime::create_purse(self).unwrap()
    }

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        Runtime::get_balance(self, purse).map_err(|_| Error::GetBalance)
    }

    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error> {
        let mint_contract_hash = self.get_mint_contract();
        self.mint_transfer(mint_contract_hash, source, target, amount)
            .map_err(|_| Error::Transfer)
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_caller(&self) -> casper_types::account::AccountHash {
        self.context.get_caller()
    }
}

impl<'a, R> AuctionProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
