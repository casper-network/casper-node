use casper_types::{
    account,
    account::AccountHash,
    auction::{Auction, EraInfo, MintProvider, RuntimeProvider, StorageProvider, SystemProvider},
    bytesrepr::{FromBytes, ToBytes},
    system_contract_errors::auction::Error,
    ApiError, CLTyped, CLValue, Key, TransferredTo, URef, BLAKE2B_DIGEST_LENGTH, U512,
};

use super::Runtime;
use crate::{
    core::execution, shared::stored_value::StoredValue, storage::global_state::StateReader,
};

/// Translates [`execution::Error`] into auction-specific [`Error`].
///
/// This function is primarily used to propagate [`Error::GasLimit`] to make sure [`Auction`]
/// contract running natively supports propagating gas limit errors without a panic.
fn to_auction_error(exec_error: execution::Error) -> Option<Error> {
    match exec_error {
        execution::Error::GasLimit => Some(Error::GasLimit),
        // There are possibly other exec errors happening but such transalation would be lossy.
        _ => None,
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
        match self
            .context
            .metered_write_gs(uref.into(), StoredValue::CLValue(cl_value))
        {
            Ok(()) => Ok(()),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::Storage)?),
        }
    }
}

impl<'a, R> SystemProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn create_purse(&mut self) -> Result<URef, Error> {
        match Runtime::create_purse(self) {
            Ok(uref) => Ok(uref),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::CreatePurseFailed)?),
        }
    }

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        match Runtime::get_balance(self, purse) {
            Ok(maybe_balance) => Ok(maybe_balance),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::GetBalance)?),
        }
    }

    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ApiError> {
        let mint_contract_hash = self.get_mint_contract();
        match self.mint_transfer(mint_contract_hash, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(api_error)) => Err(api_error),
            Err(exec_error) => Err(to_auction_error(exec_error)
                .ok_or(ApiError::Transfer)?
                .into()),
        }
    }

    fn record_era_info(&mut self, era_id: u64, era_info: EraInfo) -> Result<(), Error> {
        match Runtime::record_era_info(self, era_id, era_info) {
            Ok(()) => Ok(()),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::RecordEraInfo)?),
        }
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_caller(&self) -> Result<AccountHash, Error> {
        Ok(self.context.get_caller())
    }

    fn get_key(&self, name: &str) -> Result<Option<Key>, Error> {
        Ok(self.context.named_keys_get(name).cloned())
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
        match self.transfer_from_purse_to_account(source, target, amount, None) {
            Ok(Ok(transferred_to)) => Ok(transferred_to),
            Ok(Err(api_error)) => Err(api_error),
            Err(exec_error) => Err(to_auction_error(exec_error)
                .ok_or(ApiError::Transfer)?
                .into()),
        }
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ApiError> {
        let mint_contract_hash = self.get_mint_contract();
        match self.mint_transfer(mint_contract_hash, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(api_error)) => Err(api_error),
            Err(exec_error) => Err(to_auction_error(exec_error)
                .ok_or(ApiError::Transfer)?
                .into()),
        }
    }

    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        match self.get_balance(purse) {
            Ok(maybe_balance) => Ok(maybe_balance),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::GetBalance)?),
        }
    }

    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        let mint_contract = self.get_mint_contract();
        match self.mint_read_base_round_reward(mint_contract) {
            Ok(result) => Ok(result),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::MissingValue)?),
        }
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        let mint_contract = self.get_mint_contract();
        match self.mint_mint(mint_contract, amount) {
            Ok(result) => Ok(result),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::MintReward)?),
        }
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        let mint_contract = self.get_mint_contract();
        match self.mint_reduce_total_supply(mint_contract, amount) {
            Ok(()) => Ok(()),
            Err(exec_error) => Err(to_auction_error(exec_error).ok_or(Error::MintReward)?),
        }
    }
}

impl<'a, R> Auction for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
