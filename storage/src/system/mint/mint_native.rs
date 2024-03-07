use tracing::error;

use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{
        error::ProviderError,
        mint::{
            runtime_provider::RuntimeProvider, storage_provider::StorageProvider,
            system_provider::SystemProvider, Mint,
        },
        runtime_native::{Id, RuntimeNative},
    },
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyExt},
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{mint::Error, Caller},
    AccessRights, AddressableEntity, CLTyped, CLValue, DeployHash, Digest, Key, Phase, PublicKey,
    StoredValue, SystemEntityRegistry, TransactionHash, Transfer, TransferAddr, URef, U512,
};

impl<S> RuntimeProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller(&self) -> AccountHash {
        self.address()
    }

    fn get_immediate_caller(&self) -> Option<Caller> {
        let caller = Caller::Session {
            account_hash: PublicKey::System.to_account_hash(),
        };
        Some(caller)
    }

    fn is_called_from_standard_payment(&self) -> bool {
        false
    }

    fn get_system_entity_registry(&self) -> Result<SystemEntityRegistry, ProviderError> {
        self.tracking_copy()
            .borrow_mut()
            .get_system_entity_registry()
            .map_err(|tce| {
                error!(%tce, "unable to obtain system contract registry during transfer");
                ProviderError::SystemContractRegistry
            })
    }

    fn read_addressable_entity_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Option<AddressableEntity>, ProviderError> {
        match self
            .tracking_copy()
            .borrow_mut()
            .get_addressable_entity_by_account_hash(self.protocol_version(), account_hash)
        {
            Ok(entity) => Ok(Some(entity)),
            Err(tce) => {
                error!(%tce, "error reading addressable entity by account hash");
                Err(ProviderError::AddressableEntityByAccountHash(account_hash))
            }
        }
    }

    fn get_phase(&self) -> Phase {
        self.phase()
    }

    fn get_key(&self, name: &str) -> Option<Key> {
        self.named_keys().get(name).cloned()
    }

    fn get_approved_spending_limit(&self) -> U512 {
        self.remaining_spending_limit()
    }

    fn sub_approved_spending_limit(&mut self, amount: U512) {
        if let Some(remaining) = self.remaining_spending_limit().checked_sub(amount) {
            self.set_remaining_spending_limit(remaining);
        } else {
            error!(
                limit = %self.remaining_spending_limit(),
                spent = %amount,
                "exceeded main purse spending limit"
            );
            self.set_remaining_spending_limit(U512::zero());
        }
    }

    fn get_main_purse(&self) -> URef {
        self.addressable_entity().main_purse()
    }

    fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        self.transfer_config().is_administrator(account_hash)
    }

    fn allow_unrestricted_transfers(&self) -> bool {
        self.transfer_config().allow_unrestricted_transfers()
    }
}

impl<S> StorageProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn new_uref<T: CLTyped + ToBytes>(&mut self, value: T) -> Result<URef, Error> {
        let cl_value: CLValue = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        let uref = self
            .address_generator()
            .new_uref(AccessRights::READ_ADD_WRITE);
        self.extend_access_rights(&[uref]);
        // we are creating this key now, thus we know it is a Key::URef and we grant the creator
        // full permissions on it, thus we do not need to do validate key / validate uref access
        // before storing it.
        self.tracking_copy()
            .borrow_mut()
            .write(Key::URef(uref), StoredValue::CLValue(cl_value));
        Ok(uref)
    }

    fn read<T: CLTyped + FromBytes>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        // check access rights on uref
        if !self.access_rights().has_access_rights_to_uref(&uref) {
            return Err(Error::ForgedReference);
        }
        let key = &Key::URef(uref);
        let stored_value = match self.tracking_copy().borrow_mut().read(key) {
            Ok(Some(stored_value)) => stored_value,
            Ok(None) => return Ok(None),
            Err(_) => return Err(Error::Storage),
        };
        // by convention, we only store CLValues under Key::URef
        if let StoredValue::CLValue(value) = stored_value {
            // Only CLTyped instances should be stored as a CLValue.
            let value = CLValue::into_t(value).map_err(|_| Error::CLValue)?;
            Ok(Some(value))
        } else {
            Err(Error::CLValue)
        }
    }

    fn write_amount(&mut self, uref: URef, amount: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(amount).map_err(|_| Error::CLValue)?;
        // is the uref writeable?
        if !uref.is_writeable() {
            return Err(Error::Storage);
        }
        // check access rights on uref
        if !self.access_rights().has_access_rights_to_uref(&uref) {
            return Err(Error::ForgedReference);
        }
        self.tracking_copy()
            .borrow_mut()
            .write(Key::URef(uref), StoredValue::CLValue(cl_value));
        Ok(())
    }

    fn add<T: CLTyped + ToBytes>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.tracking_copy()
            .borrow_mut()
            .add(Key::URef(uref), StoredValue::CLValue(cl_value))
            .map_err(|_| Error::Storage)?;
        Ok(())
    }

    fn read_balance(&mut self, uref: URef) -> Result<Option<U512>, Error> {
        match self
            .tracking_copy()
            .borrow_mut()
            .get_purse_balance(Key::Balance(uref.addr()))
        {
            Ok(motes) => Ok(Some(motes.value())),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write_balance(&mut self, uref: URef, balance: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(balance).map_err(|_| Error::CLValue)?;
        self.tracking_copy()
            .borrow_mut()
            .write(Key::Balance(uref.addr()), StoredValue::CLValue(cl_value));
        Ok(())
    }

    fn add_balance(&mut self, uref: URef, value: U512) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.tracking_copy()
            .borrow_mut()
            .add(Key::Balance(uref.addr()), StoredValue::CLValue(cl_value))
            .map_err(|_| Error::Storage)?;
        Ok(())
    }
}

impl<S> SystemProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn record_transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), Error> {
        if self.phase() != Phase::Session {
            return Ok(());
        }
        let transfer_addr = TransferAddr::new(self.address_generator().create_address());
        let key = Key::Transfer(transfer_addr); // <-- a new key variant needed to deal w/ versioned transaction hash
                                                //let transaction_hash = self.transaction_hash();
        let transfer = {
            // the below line is incorrect; new transaction hash is not currently supported here
            // ...the transfer struct needs to be upgraded to TransactionHash
            let deploy_hash = match self.id() {
                Id::Transaction(transaction) => {
                    match transaction {
                        TransactionHash::Deploy(deploy_hash) => *deploy_hash,
                        TransactionHash::V1(hash) => {
                            // TODO: this is bogus...update when new transfer record is available
                            let hash = hash.inner();
                            DeployHash::new(*hash)
                        }
                    }
                }
                Id::Seed(seed) => DeployHash::new(Digest::hash(seed)),
            };
            let from: AccountHash = self.get_caller();
            let fee: U512 = U512::zero();
            Transfer::new(deploy_hash, from, maybe_to, source, target, amount, fee, id)
        };
        self.push_transfer(transfer_addr);

        self.tracking_copy()
            .borrow_mut()
            .write(key, StoredValue::Transfer(transfer));
        Ok(())
    }
}

impl<S> Mint for RuntimeNative<S> where S: StateReader<Key, StoredValue, Error = GlobalStateError> {}
