use casperlabs_types::{
    auction::{AuctionProvider, MintProvider, RuntimeProvider, StorageProvider, SystemProvider},
    bytesrepr::{FromBytes, ToBytes},
    runtime_args,
    system_contract_errors::auction::Error,
    CLTyped, CLValue, Key, RuntimeArgs, URef, U512,
};

use super::Runtime;
use crate::components::contract_runtime::{
    core::execution, shared::stored_value::StoredValue, storage::global_state::StateReader,
};

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;

    fn get_key(&mut self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Self::Error> {
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
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Self::Error> {
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
    type Error = Error;
    fn create_purse(&mut self) -> URef {
        Runtime::create_purse(self).unwrap()
    }
    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Self::Error> {
        Runtime::get_balance(self, purse).map_err(|_| Error::GetBalance)
    }
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Self::Error> {
        let mint_contract_hash = self.get_mint_contract();
        self.mint_transfer(mint_contract_hash, source, target, amount)
            .map_err(|_| Error::Transfer)
    }
}

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;

    fn bond(&mut self, amount: U512, purse: URef) -> Result<(URef, U512), Self::Error> {
        const ARG_AMOUNT: &str = "amount";
        const ARG_PURSE: &str = "purse";

        let args_values: RuntimeArgs = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PURSE => purse,
        };

        let mint_contract_hash = self.get_mint_contract();

        let result = self
            .call_contract(mint_contract_hash, "bond", args_values)
            .map_err(|_| Error::Bonding)?;
        Ok(result.into_t().map_err(|_| Error::Bonding)?)
    }

    fn unbond(&mut self, amount: U512) -> Result<(URef, U512), Self::Error> {
        const ARG_AMOUNT: &str = "amount";

        let args_values: RuntimeArgs = runtime_args! {
            ARG_AMOUNT => amount,
        };

        let mint_contract_hash = self.get_mint_contract();

        let result = self
            .call_contract(mint_contract_hash, "unbond", args_values)
            .map_err(|_| Error::Unbonding)?;
        Ok(result.into_t().map_err(|_| Error::Unbonding)?)
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_caller(&self) -> casperlabs_types::account::AccountHash {
        self.context.get_caller()
    }
}

impl<'a, R> AuctionProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
