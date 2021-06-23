use std::collections::BTreeSet;

use casper_types::{
    account,
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{
        auction::{
            AccountProvider, Auction, Bid, EraInfo, Error, MintProvider, RuntimeProvider,
            StorageProvider, SystemProvider, UnbondingPurse,
        },
        CallStackElement,
    },
    CLTyped, CLValue, EraId, Key, KeyTag, TransferredTo, URef, BLAKE2B_DIGEST_LENGTH, U512,
};

use super::Runtime;
use crate::{
    core::execution, shared::stored_value::StoredValue, storage::global_state::StateReader,
};

impl From<execution::Error> for Option<Error> {
    fn from(exec_error: execution::Error) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure [`Auction`]
            // contract running natively supports propagating gas limit errors without a panic.
            execution::Error::GasLimit => Some(Error::GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        match self.context.read_gs(&uref.into()) {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                Ok(Some(cl_value.into_t().map_err(|_| Error::CLValue)?))
            }
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(None),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(execution::Error::GasLimit) => Err(Error::GasLimit),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .metered_write_gs(uref.into(), StoredValue::CLValue(cl_value))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn read_bid(&mut self, account_hash: &AccountHash) -> Result<Option<Bid>, Error> {
        match self.context.read_gs(&Key::Bid(*account_hash)) {
            Ok(Some(StoredValue::Bid(bid))) => Ok(Some(*bid)),
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(None),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(execution::Error::GasLimit) => Err(Error::GasLimit),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write_bid(&mut self, account_hash: AccountHash, bid: Bid) -> Result<(), Error> {
        self.context
            .metered_write_gs_unsafe(Key::Bid(account_hash), StoredValue::Bid(Box::new(bid)))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn read_withdraw(&mut self, account_hash: &AccountHash) -> Result<Vec<UnbondingPurse>, Error> {
        match self.context.read_gs(&Key::Withdraw(*account_hash)) {
            Ok(Some(StoredValue::Withdraw(unbonding_purses))) => Ok(unbonding_purses),
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(Vec::new()),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(execution::Error::GasLimit) => Err(Error::GasLimit),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write_withdraw(
        &mut self,
        account_hash: AccountHash,
        unbonding_purses: Vec<UnbondingPurse>,
    ) -> Result<(), Error> {
        self.context
            .metered_write_gs_unsafe(
                Key::Withdraw(account_hash),
                StoredValue::Withdraw(unbonding_purses),
            )
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }
}

impl<'a, R> SystemProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn create_purse(&mut self) -> Result<URef, Error> {
        Runtime::create_purse(self).map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::CreatePurseFailed)
        })
    }

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        Runtime::get_balance(self, purse)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::GetBalance))
    }

    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error> {
        let mint_contract_hash = self.get_mint_contract();
        match self.mint_transfer(mint_contract_hash, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            // NOTE: Error below is a mint error which is lossy conversion. In calling code we map
            // it anyway into more specific error.
            Ok(Err(_mint_error)) => Err(Error::Transfer),
            Err(exec_error) => Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer)),
        }
    }

    fn record_era_info(&mut self, era_id: EraId, era_info: EraInfo) -> Result<(), Error> {
        Runtime::record_era_info(self, era_id, era_info)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::RecordEraInfo))
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

    fn get_immediate_caller(&self) -> Option<&CallStackElement> {
        let call_stack = self.call_stack();
        let mut call_stack_iter = call_stack.iter().rev();
        call_stack_iter.next()?;
        call_stack_iter.next()
    }

    fn named_keys_get(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error> {
        self.context.get_keys(key_tag).map_err(|_| Error::Storage)
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
    ) -> Result<TransferredTo, Error> {
        match self.transfer_from_purse_to_account(source, target, amount, None) {
            Ok(Ok(transferred_to)) => Ok(transferred_to),
            Ok(Err(_api_error)) => Err(Error::Transfer),
            Err(exec_error) => Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer)),
        }
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error> {
        let mint_contract_hash = self.get_mint_contract();
        match self.mint_transfer(mint_contract_hash, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_mint_error)) => Err(Error::Transfer),
            Err(exec_error) => Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer)),
        }
    }

    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        self.get_balance(purse)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::GetBalance))
    }

    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        let mint_contract = self.get_mint_contract();
        self.mint_read_base_round_reward(mint_contract)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue))
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        let mint_contract = self.get_mint_contract();
        self.mint_mint(mint_contract, amount)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        let mint_contract = self.get_mint_contract();
        self.mint_reduce_total_supply(mint_contract, amount)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))
    }
}

impl<'a, R> AccountProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_main_purse(&self) -> Result<URef, Error> {
        // NOTE: This violates security as system contract is a contract entrypoint and normal
        // "get_main_purse" won't work for security reasons. But since we're not running it as a
        // WASM contract, and purses are going to be removed anytime soon, we're making this
        // exception here.
        Ok(Runtime::context(self).account().main_purse())
    }
}

impl<'a, R> Auction for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
