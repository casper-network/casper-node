use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{mint::Error, CallStackElement},
    CLTyped, CLValue, Key, Phase, StoredValue, URef, U512,
};

use super::Runtime;
use crate::{
    core::{engine_state::SystemContractRegistry, execution},
    storage::global_state::StateReader,
    system::mint::{
        runtime_provider::RuntimeProvider, storage_provider::StorageProvider,
        system_provider::SystemProvider, Mint,detail
    },
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

    fn get_immediate_caller(&self) -> Option<&CallStackElement> {
        Runtime::<'a, R>::get_immediate_caller(self)
    }

    fn get_phase(&self) -> Phase {
        self.context.phase()
    }

    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error> {
        self.context
            .put_key(name.to_string(), key)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::PutKey))
    }

    fn get_key(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn get_approved_spending_limit(&self) -> U512 {
        self.context.remaining_spending_limit()
    }

    fn sub_approved_spending_limit(&mut self, transferred: U512) {
        // We're ignoring the result here since we always check first
        // if there is still enough spending limit left.
        self.context.subtract_amount_spent(transferred);
    }

    fn get_main_purse(&self) -> URef {
        self.context.account().main_purse()
    }

    fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        self.config.is_administrator(account_hash)
    }

    fn allow_unrestricted_transfers(&self) -> bool {
        self.config.allow_unrestricted_transfers()
    }

    fn get_system_contract_registry(&self) -> Result<SystemContractRegistry, execution::Error> {
        self.context.system_contract_registry()
    }

    fn is_called_from_standard_payment(&self) -> bool {
        self.context.phase() == Phase::Payment && self.module.is_none()
    }

    fn read_account(
        &mut self,
        account_hash: &AccountHash,
    ) -> Result<Option<StoredValue>, execution::Error> {
        self.context.read_account(&Key::Account(*account_hash))
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

    fn read_balance(&mut self, uref: URef) -> Result<Option<U512>, Error> {
        let maybe_value = self
            .context
            .read_gs_direct(&Key::Balance(uref.addr()))
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

    fn write_balance(&mut self, uref: URef, balance: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(balance).map_err(|_| Error::CLValue)?;
        self.context
            .metered_write_gs_unsafe(Key::Balance(uref.addr()), StoredValue::CLValue(cl_value))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn add_balance(&mut self, uref: URef, value: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .metered_add_gs_unsafe(Key::Balance(uref.addr()), StoredValue::CLValue(cl_value))
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
    /// Burns native tokens.
    fn burn(&mut self, purse: URef, amount: U512) -> Result<(), Error> {
        let purse_key = Key::URef(purse);
        self.context.validate_writeable(&purse_key).map_err(|_| Error::InvalidAccessRights)?;
        self.context.validate_key(&purse_key).map_err(|_| Error::InvalidURef)?;

        let source_balance: U512 = match self.read_balance(purse)? {
            Some(source_balance) => source_balance,
            None => return Err(Error::PurseNotFound),
        };

        let new_balance = match source_balance.checked_sub(amount) {
            Some(value) => value,
            None => U512::zero()
        };

        // source_balance is >= than new_balance 
        // this should block user from reducing totaly supply beyond what they own
        let burned_amount = source_balance - new_balance;

        self.write_balance(purse, new_balance)?;
        detail::reduce_total_supply_unchecked(self, burned_amount)
    }
}
