use casper_types::{
    account,
    account::AccountHash,
    auction::{Auction, MintProvider, RuntimeProvider, StorageProvider, SystemProvider},
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::Error,
    ApiError, CLTyped, CLValue, Key, TransferredTo, URef, BLAKE2B_DIGEST_LENGTH, U512,
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
            .metered_write_gs(uref.into(), StoredValue::CLValue(cl_value))
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
    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }

    fn get_key(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn put_key(&mut self, name: &str, key: Key) {
        self.context
            .put_key(name.to_string(), key)
            .expect("should put key")
    }

    fn blake2b<T: AsRef<[u8]>>(&self, data: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        account::blake2b(data)
    }
}

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, ApiError> {
        self.transfer_from_purse_to_account(source, target, amount)
            .expect("should transfer from purse to account")
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ()> {
        let mint_contract_key = self.get_mint_contract();
        if self
            .mint_transfer(mint_contract_key, source, target, amount)
            .is_ok()
        {
            Ok(())
        } else {
            Err(())
        }
    }

    fn balance(&mut self, purse: URef) -> Option<U512> {
        self.get_balance(purse).expect("should get balance")
    }

    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        let mint_contract = self.get_mint_contract();
        self.mint_read_base_round_reward(mint_contract)
            .map_err(|_| Error::MissingValue)
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        let mint_contract = self.get_mint_contract();
        self.mint_mint(mint_contract, amount)
            .map_err(|_| Error::MintReward)
    }
}

impl<'a, R> Auction for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
