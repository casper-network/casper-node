use tracing::error;

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{
        error::ProviderError,
        mint::{
            runtime_provider::RuntimeProvider, storage_provider::StorageProvider,
            system_provider::SystemProvider, Mint,
        },
    },
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{mint::Error, Caller},
    CLTyped, CLValue, Key, Phase, RuntimeFootprint, StoredValue, SystemHashRegistry, URef, U512,
};

use super::Runtime;
use crate::execution::ExecError;

impl From<ExecError> for Option<Error> {
    fn from(exec_error: ExecError) -> Self {
        match exec_error {
            // This is used to propagate [`ExecError::GasLimit`] to make sure [`Mint`]
            // contract running natively supports propagating gas limit errors without a panic.
            ExecError::GasLimit => Some(Error::GasLimit),
            ExecError::ForgedReference(_) => Some(Error::ForgedReference),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

impl<R> RuntimeProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller(&self) -> AccountHash {
        self.context.get_initiator()
    }

    fn get_immediate_caller(&self) -> Option<Caller> {
        Runtime::<'_, R>::get_immediate_caller(self).cloned()
    }

    fn is_called_from_standard_payment(&self) -> bool {
        self.context.phase() == Phase::Payment && self.module.is_none()
    }

    fn get_system_entity_registry(&self) -> Result<SystemHashRegistry, ProviderError> {
        self.context.system_entity_registry().map_err(|err| {
            error!(%err, "unable to obtain system entity registry during transfer");
            ProviderError::SystemEntityRegistry
        })
    }

    fn runtime_footprint_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Option<RuntimeFootprint>, ProviderError> {
        self.context
            .runtime_footprint_by_account_hash(account_hash)
            .map_err(|err| {
                error!(%err, "error getting runtime footprint by account hash");
                ProviderError::AccountHash(account_hash)
            })
    }

    fn get_phase(&self) -> Phase {
        self.context.phase()
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

    fn get_main_purse(&self) -> Option<URef> {
        self.context.runtime_footprint().borrow().main_purse()
    }

    fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        self.context.engine_config().is_administrator(account_hash)
    }

    fn allow_unrestricted_transfers(&self) -> bool {
        self.context.engine_config().allow_unrestricted_transfers()
    }

    /// Validate URef against context access rights.
    fn is_valid_uref(&self, uref: &URef) -> bool {
        self.context.access_rights().has_access_rights_to_uref(uref)
    }
}

impl<R> StorageProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
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

    fn write_amount(&mut self, uref: URef, amount: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(amount).map_err(|_| Error::CLValue)?;
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

    fn total_balance(&mut self, purse: URef) -> Result<U512, Error> {
        Runtime::total_balance(self, purse)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn available_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        Runtime::available_balance(self, purse)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
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

impl<R> SystemProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
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

impl<R> Mint for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn purse_exists(&mut self, uref: URef) -> Result<bool, Error> {
        let maybe_value = self
            .context
            .read_gs(&Key::Balance(uref.addr()))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))?;
        match maybe_value {
            Some(StoredValue::CLValue(value)) => Ok(*value.cl_type() == U512::cl_type()),
            Some(_non_cl_value) => Err(Error::CLValue),
            None => Ok(false),
        }
    }
}
