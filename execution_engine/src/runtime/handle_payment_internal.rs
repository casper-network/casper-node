use casper_storage::global_state::{error::Error as GlobalStateError, state::StateReader};
use std::collections::BTreeSet;

use casper_types::{
    account::AccountHash, addressable_entity::NamedKeyAddr, system::handle_payment::Error,
    BlockTime, CLValue, FeeHandling, Key, Phase, RefundHandling, StoredValue, TransferredTo, URef,
    U512,
};

use casper_storage::system::handle_payment::{
    mint_provider::MintProvider, runtime_provider::RuntimeProvider,
    storage_provider::StorageProvider, HandlePayment,
};

use crate::{execution, runtime::Runtime};

impl From<execution::Error> for Option<Error> {
    fn from(exec_error: execution::Error) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure
            // [`HandlePayment`] contract running natively supports propagating gas limit
            // errors without a panic.
            execution::Error::GasLimit => Some(Error::GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

// TODO: Update MintProvider to better handle errors
impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, Error> {
        match self.transfer_from_purse_to_account_hash(source, target, amount, None) {
            Ok(Ok(transferred_to)) => Ok(transferred_to),
            Ok(Err(_mint_error)) => Err(Error::Transfer),
            Err(exec_error) => Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer)),
        }
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error> {
        let mint_contract_key = match self.get_mint_contract() {
            Ok(mint_hash) => mint_hash,
            Err(exec_error) => {
                return Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer));
            }
        };
        match self.mint_transfer(mint_contract_key, None, source, target, amount, None) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(_mint_error)) => Err(Error::Transfer),
            Err(exec_error) => Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer)),
        }
    }

    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        self.get_balance(purse)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::GetBalance))
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        let mint_contract_key = match self.get_mint_contract() {
            Ok(mint_hash) => mint_hash,
            Err(exec_error) => {
                return Err(<Option<Error>>::from(exec_error).unwrap_or(Error::Transfer));
            }
        };
        if let Err(exec_error) = self.mint_reduce_total_supply(mint_contract_key, amount) {
            Err(<Option<Error>>::from(exec_error).unwrap_or(Error::ReduceTotalSupply))
        } else {
            Ok(())
        }
    }
}

// TODO: Update RuntimeProvider to better handle errors
impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_key(&mut self, name: &str) -> Option<Key> {
        match self.context.named_keys_get(name).cloned() {
            None => {
                let entity_addr = match self.context.get_entity_key().as_entity_addr() {
                    Some(addr) => addr,
                    None => return None,
                };
                let key = if let Ok(addr) =
                    NamedKeyAddr::new_from_string(entity_addr, name.to_string())
                {
                    Key::NamedKey(addr)
                } else {
                    return None;
                };
                if let Ok(Some(StoredValue::NamedKey(value))) = self.context.read_gs(&key) {
                    value.get_key().ok()
                } else {
                    None
                }
            }
            Some(key) => Some(key),
        }
    }

    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error> {
        self.context
            .put_key(name.to_string(), key)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::PutKey))
    }

    fn remove_key(&mut self, name: &str) -> Result<(), Error> {
        self.context
            .remove_key(name)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::RemoveKey))
    }

    fn get_phase(&self) -> Phase {
        self.context.phase()
    }

    fn get_block_time(&self) -> BlockTime {
        self.context.get_blocktime()
    }

    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }

    fn refund_handling(&self) -> &RefundHandling {
        self.context.engine_config().refund_handling()
    }

    fn fee_handling(&self) -> FeeHandling {
        self.context.engine_config().fee_handling()
    }

    fn administrative_accounts(&self) -> &BTreeSet<AccountHash> {
        self.context.engine_config().administrative_accounts()
    }
}

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn write_balance(&mut self, purse_uref: URef, amount: U512) -> Result<(), Error> {
        let cl_amount = CLValue::from_t(amount).map_err(|_| Error::Storage)?;
        self.context
            .metered_write_gs_unsafe(Key::Balance(purse_uref.addr()), cl_amount)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))?;
        Ok(())
    }
}

impl<'a, R> HandlePayment for Runtime<'a, R> where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>
{
}
