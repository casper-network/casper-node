use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{
        handle_payment::{
            mint_provider::MintProvider, runtime_provider::RuntimeProvider,
            storage_provider::StorageProvider, HandlePayment,
        },
        mint::Mint,
        runtime_native::RuntimeNative,
    },
    tracking_copy::TrackingCopyEntityExt,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{NamedKeyAddr, NamedKeyValue},
    system::handle_payment::Error,
    AccessRights, CLValue, FeeHandling, GrantedAccess, Key, Phase, RefundHandling, StoredValue,
    TransferredTo, URef, U512,
};
use std::collections::BTreeSet;
use tracing::error;

impl<S> MintProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, Error> {
        let target_key = Key::Account(target);
        let target_uref = match self.tracking_copy().borrow_mut().read(&target_key) {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                let entity_key = CLValue::into_t::<Key>(cl_value)
                    .map_err(|_| Error::FailedTransferToAccountPurse)?;
                // get entity
                let target_uref = {
                    if let Ok(Some(StoredValue::AddressableEntity(entity))) =
                        self.tracking_copy().borrow_mut().read(&entity_key)
                    {
                        entity.main_purse_add_only()
                    } else {
                        return Err(Error::Transfer);
                    }
                };
                target_uref
            } // entity exists
            Ok(Some(StoredValue::Account(account))) => {
                if self.config().enable_addressable_entity() {
                    self.tracking_copy()
                        .borrow_mut()
                        .migrate_account(target, self.protocol_version())
                        .map_err(|_| Error::Transfer)?;
                }

                account.main_purse_add_only()
            }
            Ok(_) | Err(_) => return Err(Error::Transfer),
        };

        // source and target are the same, noop
        if source.with_access_rights(AccessRights::ADD) == target_uref {
            return Ok(TransferredTo::ExistingAccount);
        }

        // Temporarily grant ADD access to target if it is not already present.
        let granted_access = self.access_rights_mut().grant_access(target_uref);

        let transfered = self
            .transfer_purse_to_purse(source, target_uref, amount)
            .is_ok();

        // if ADD access was temporarily granted, remove it.
        if let GrantedAccess::Granted {
            uref_addr,
            newly_granted_access_rights,
        } = granted_access
        {
            self.access_rights_mut()
                .remove_access(uref_addr, newly_granted_access_rights)
        }

        if transfered {
            Ok(TransferredTo::ExistingAccount)
        } else {
            Err(Error::Transfer)
        }
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error> {
        // system purses do not have holds on them
        match self.transfer(None, source, target, amount, None) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::Transfer)
            }
        }
    }

    fn available_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        match <Self as Mint>::balance(self, purse) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::GetBalance)
            }
        }
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        match <Self as Mint>::reduce_total_supply(self, amount) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::ReduceTotalSupply)
            }
        }
    }
}

impl<S> RuntimeProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_key(&mut self, name: &str) -> Option<Key> {
        self.named_keys().get(name).cloned()
    }

    fn put_key(&mut self, name: &str, key: Key) -> Result<(), Error> {
        let name = name.to_string();
        match self.context_key() {
            Key::Account(_) | Key::Hash(_) => {
                let name: String = name.clone();
                let value = CLValue::from_t((name.clone(), key)).map_err(|_| Error::PutKey)?;
                let named_key_value = StoredValue::CLValue(value);
                self.tracking_copy()
                    .borrow_mut()
                    .add(*self.context_key(), named_key_value)
                    .map_err(|_| Error::PutKey)?;
                self.named_keys_mut().insert(name, key);
                Ok(())
            }
            Key::AddressableEntity(entity_addr) => {
                let named_key_value = StoredValue::NamedKey(
                    NamedKeyValue::from_concrete_values(key, name.clone())
                        .map_err(|_| Error::PutKey)?,
                );
                let named_key_addr = NamedKeyAddr::new_from_string(*entity_addr, name.clone())
                    .map_err(|_| Error::PutKey)?;
                let named_key = Key::NamedKey(named_key_addr);
                // write to both tracking copy and in-mem named keys cache
                self.tracking_copy()
                    .borrow_mut()
                    .write(named_key, named_key_value);
                self.named_keys_mut().insert(name, key);
                Ok(())
            }
            _ => Err(Error::UnexpectedKeyVariant),
        }
    }

    fn remove_key(&mut self, name: &str) -> Result<(), Error> {
        self.named_keys_mut().remove(name);
        match self.context_key() {
            Key::AddressableEntity(entity_addr) => {
                let named_key_addr = NamedKeyAddr::new_from_string(*entity_addr, name.to_string())
                    .map_err(|_| Error::RemoveKey)?;
                let key = Key::NamedKey(named_key_addr);
                let value = self
                    .tracking_copy()
                    .borrow_mut()
                    .read(&key)
                    .map_err(|_| Error::RemoveKey)?;
                if let Some(StoredValue::NamedKey(_)) = value {
                    self.tracking_copy().borrow_mut().prune(key);
                }
            }
            Key::Hash(_) => {
                let mut contract = self
                    .tracking_copy()
                    .borrow_mut()
                    .read(self.context_key())
                    .map_err(|_| Error::RemoveKey)?
                    .ok_or(Error::RemoveKey)?
                    .as_contract()
                    .ok_or(Error::RemoveKey)?
                    .clone();

                if contract.remove_named_key(name).is_none() {
                    return Ok(());
                }

                self.tracking_copy()
                    .borrow_mut()
                    .write(*self.context_key(), StoredValue::Contract(contract))
            }
            Key::Account(_) => {
                let account = {
                    let mut account = match self
                        .tracking_copy()
                        .borrow_mut()
                        .read(self.context_key())
                        .map_err(|_| Error::RemoveKey)?
                    {
                        Some(StoredValue::Account(account)) => account,
                        Some(_) | None => return Err(Error::UnexpectedKeyVariant),
                    };
                    account.named_keys_mut().remove(name);
                    account
                };
                self.tracking_copy()
                    .borrow_mut()
                    .write(*self.context_key(), StoredValue::Account(account));
            }
            _ => return Err(Error::UnexpectedKeyVariant),
        }

        Ok(())
    }

    fn get_phase(&self) -> Phase {
        self.phase()
    }

    fn get_caller(&self) -> AccountHash {
        self.address()
    }

    fn refund_handling(&self) -> RefundHandling {
        *self.config().refund_handling()
    }

    fn fee_handling(&self) -> FeeHandling {
        *self.config().fee_handling()
    }

    fn administrative_accounts(&self) -> BTreeSet<AccountHash> {
        self.transfer_config().administrative_accounts()
    }
}

impl<S> StorageProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn write_balance(&mut self, purse_uref: URef, amount: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(amount).map_err(|_| Error::Storage)?;
        self.tracking_copy().borrow_mut().write(
            Key::Balance(purse_uref.addr()),
            StoredValue::CLValue(cl_value),
        );
        Ok(())
    }
}

impl<S> HandlePayment for RuntimeNative<S> where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>
{
}
