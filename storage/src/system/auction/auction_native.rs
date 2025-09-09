use crate::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{
        auction::{
            providers::{AccountProvider, MintProvider, RuntimeProvider, StorageProvider},
            Auction,
        },
        mint::Mint,
        runtime_native::RuntimeNative,
    },
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{
        auction::{BidAddr, BidKind, EraInfo, Error, Unbond, UnbondEra, UnbondKind},
        mint,
    },
    AccessRights, CLTyped, CLValue, Key, KeyTag, PublicKey, StoredValue, URef, U512,
};
use std::collections::BTreeSet;
use tracing::{debug, error};

impl<S> RuntimeProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller(&self) -> AccountHash {
        self.address()
    }

    fn is_allowed_session_caller(&self, account_hash: &AccountHash) -> bool {
        if self.get_caller() == PublicKey::System.to_account_hash() {
            return true;
        }

        account_hash == &self.address()
    }

    fn is_valid_uref(&self, uref: URef) -> bool {
        self.access_rights().has_access_rights_to_uref(&uref)
    }

    fn named_keys_get(&self, name: &str) -> Option<Key> {
        self.named_keys().get(name).cloned()
    }

    fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error> {
        self.tracking_copy()
            .borrow_mut()
            .get_keys(key_tag)
            .map_err(|error| {
                error!(%key_tag, "RuntimeProvider::get_keys: {:?}", error);
                Error::Storage
            })
    }

    fn get_keys_by_prefix(&mut self, prefix: &[u8]) -> Result<Vec<Key>, Error> {
        self.tracking_copy()
            .borrow_mut()
            .reader()
            .keys_with_prefix(prefix)
            .map_err(|error| {
                error!("RuntimeProvider::get_keys_by_prefix: {:?}", error);
                Error::Storage
            })
    }

    fn delegator_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error> {
        let delegated_accounts = {
            let prefix = bid_addr.delegated_account_prefix()?;
            let keys = self.get_keys_by_prefix(&prefix).map_err(|err| {
                error!("RuntimeProvider::delegator_count {:?}", err);
                Error::Storage
            })?;
            keys.len()
        };
        let delegated_purses = {
            let prefix = bid_addr.delegated_purse_prefix()?;
            let keys = self.get_keys_by_prefix(&prefix).map_err(|err| {
                error!("RuntimeProvider::delegator_count {:?}", err);
                Error::Storage
            })?;
            keys.len()
        };
        Ok(delegated_accounts.saturating_add(delegated_purses))
    }

    fn reservation_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error> {
        let reserved_accounts = {
            let reservation_prefix = bid_addr.reserved_account_prefix()?;
            let reservation_keys = self
                .get_keys_by_prefix(&reservation_prefix)
                .map_err(|err| {
                    error!("RuntimeProvider::reservation_count {:?}", err);
                    Error::Storage
                })?;
            reservation_keys.len()
        };
        let reserved_purses = {
            let reservation_prefix = bid_addr.reserved_purse_prefix()?;
            let reservation_keys = self
                .get_keys_by_prefix(&reservation_prefix)
                .map_err(|err| {
                    error!("RuntimeProvider::reservation_count {:?}", err);
                    Error::Storage
                })?;
            reservation_keys.len()
        };
        Ok(reserved_accounts.saturating_add(reserved_purses))
    }

    fn used_reservation_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error> {
        let reservation_account_prefix = bid_addr.reserved_account_prefix()?;
        let reservation_purse_prefix = bid_addr.reserved_purse_prefix()?;

        let mut reservation_keys = self
            .get_keys_by_prefix(&reservation_account_prefix)
            .map_err(|exec_error| {
                error!("RuntimeProvider::reservation_count {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })?;

        let more = self
            .get_keys_by_prefix(&reservation_purse_prefix)
            .map_err(|exec_error| {
                error!("RuntimeProvider::reservation_count {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })?;

        reservation_keys.extend(more);

        let mut used = 0;
        for reservation_key in reservation_keys {
            if let Key::BidAddr(BidAddr::ReservedDelegationAccount {
                validator,
                delegator,
            }) = reservation_key
            {
                let key_to_check = Key::BidAddr(BidAddr::DelegatedAccount {
                    validator,
                    delegator,
                });
                if let Ok(Some(_)) = self.read_bid(&key_to_check) {
                    used += 1;
                }
            }
            if let Key::BidAddr(BidAddr::ReservedDelegationPurse {
                validator,
                delegator,
            }) = reservation_key
            {
                let key_to_check = Key::BidAddr(BidAddr::DelegatedPurse {
                    validator,
                    delegator,
                });
                if let Ok(Some(_)) = self.read_bid(&key_to_check) {
                    used += 1;
                }
            }
        }
        Ok(used)
    }

    fn vesting_schedule_period_millis(&self) -> u64 {
        self.vesting_schedule_period_millis()
    }

    fn allow_auction_bids(&self) -> bool {
        self.allow_auction_bids()
    }

    fn should_compute_rewards(&self) -> bool {
        self.compute_rewards()
    }
}

impl<S> StorageProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error> {
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

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        // is the uref writeable?
        if !uref.is_writeable() {
            error!("uref not writeable {}", uref);
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

    fn read_bid(&mut self, key: &Key) -> Result<Option<BidKind>, Error> {
        match self.tracking_copy().borrow_mut().read(key) {
            Ok(Some(StoredValue::BidKind(bid_kind))) => Ok(Some(bid_kind)),
            Ok(Some(_)) => {
                error!("StorageProvider::read_bid: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(TrackingCopyError::BytesRepr(_)) => Err(Error::Serialization),
            Err(err) => {
                error!("StorageProvider::read_bid: {:?}", err);
                Err(Error::Storage)
            }
        }
    }

    fn write_bid(&mut self, key: Key, bid_kind: BidKind) -> Result<(), Error> {
        let stored_value = StoredValue::BidKind(bid_kind);

        // Charge for amount as measured by serialized length
        // let bytes_count = stored_value.serialized_length();
        // self.charge_gas_storage(bytes_count)?;

        self.tracking_copy().borrow_mut().write(key, stored_value);
        Ok(())
    }

    fn read_unbond(&mut self, bid_addr: BidAddr) -> Result<Option<Unbond>, Error> {
        match self
            .tracking_copy()
            .borrow_mut()
            .read(&Key::BidAddr(bid_addr))
        {
            Ok(Some(StoredValue::BidKind(BidKind::Unbond(unbond)))) => Ok(Some(*unbond)),
            Ok(Some(_)) => {
                error!("StorageProvider::read_unbonds: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(TrackingCopyError::BytesRepr(_)) => Err(Error::Serialization),
            Err(err) => {
                error!("StorageProvider::read_unbonds: {:?}", err);
                Err(Error::Storage)
            }
        }
    }

    fn write_unbond(&mut self, bid_addr: BidAddr, unbond: Option<Unbond>) -> Result<(), Error> {
        let unbond_key = Key::BidAddr(bid_addr);
        match unbond {
            Some(unbond) => {
                self.tracking_copy().borrow_mut().write(
                    unbond_key,
                    StoredValue::BidKind(BidKind::Unbond(Box::new(unbond))),
                );
            }
            None => {
                self.tracking_copy().borrow_mut().prune(unbond_key);
            }
        }
        Ok(())
    }

    fn record_era_info(&mut self, era_info: EraInfo) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidContext);
        }
        self.tracking_copy()
            .borrow_mut()
            .write(Key::EraSummary, StoredValue::EraInfo(era_info));
        Ok(())
    }

    fn prune_bid(&mut self, bid_addr: BidAddr) {
        self.tracking_copy().borrow_mut().prune(bid_addr.into());
    }
}

impl<S> MintProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn unbond(&mut self, unbond_kind: &UnbondKind, unbond_era: &UnbondEra) -> Result<(), Error> {
        let (purse, maybe_account_hash) = match unbond_kind {
            UnbondKind::Validator(pk) | UnbondKind::DelegatedPublicKey(pk) => {
                let account_hash = pk.to_account_hash();
                // Do a migration if the account hasn't been migrated yet. This is just a read if it
                // has been migrated already.
                self.tracking_copy()
                    .borrow_mut()
                    .migrate_account(account_hash, self.protocol_version())
                    .map_err(|error| {
                        error!(
                            "MintProvider::unbond: couldn't migrate account: {:?}",
                            error
                        );
                        Error::Storage
                    })?;

                let maybe_value = self
                    .tracking_copy()
                    .borrow_mut()
                    .read(&Key::Account(account_hash))
                    .map_err(|error| {
                        error!("MintProvider::unbond: {:?}", error);
                        Error::Storage
                    })?;

                match maybe_value {
                    Some(StoredValue::Account(account)) => {
                        (account.main_purse(), Some(account_hash))
                    }
                    Some(StoredValue::CLValue(cl_value)) => {
                        let entity_key: Key = cl_value.into_t().map_err(|_| Error::CLValue)?;
                        let maybe_value = self
                            .tracking_copy()
                            .borrow_mut()
                            .read(&entity_key)
                            .map_err(|error| {
                                error!("MintProvider::unbond: {:?}", error);
                                Error::Storage
                            })?;
                        match maybe_value {
                            Some(StoredValue::AddressableEntity(entity)) => {
                                (entity.main_purse(), Some(account_hash))
                            }
                            Some(_cl_value) => return Err(Error::CLValue),
                            None => return Err(Error::InvalidPublicKey),
                        }
                    }
                    Some(_cl_value) => return Err(Error::CLValue),
                    None => return Err(Error::InvalidPublicKey),
                }
            }
            UnbondKind::DelegatedPurse(addr) => {
                let purse = URef::new(*addr, AccessRights::READ_ADD_WRITE);
                match self.balance(purse) {
                    Ok(Some(_)) => (purse, None),
                    Ok(None) => return Err(Error::MissingPurse),
                    Err(err) => {
                        error!("MintProvider::unbond: {:?}", err);
                        return Err(Error::Unbonding);
                    }
                }
            }
        };

        self.mint_transfer_direct(
            maybe_account_hash,
            *unbond_era.bonding_purse(),
            purse,
            *unbond_era.amount(),
            None,
        )
        .map_err(|_| Error::Transfer)?
        .map_err(|_| Error::Transfer)?;
        Ok(())
    }

    fn mint_transfer_direct(
        &mut self,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<Result<(), mint::Error>, Error> {
        let addr = if let Some(uref) = self.runtime_footprint().main_purse() {
            uref.addr()
        } else {
            return Err(Error::InvalidContext);
        };
        if !(addr == source.addr() || self.get_caller() == PublicKey::System.to_account_hash()) {
            return Err(Error::InvalidCaller);
        }

        // let gas_counter = self.gas_counter();
        self.extend_access_rights(&[source, target.into_add()]);

        match self.transfer(to, source, target, amount, id) {
            Ok(ret) => Ok(Ok(ret)),
            Err(err) => {
                error!("{}", err);
                Err(Error::Transfer)
            }
        }
    }

    fn mint_into_existing_purse(
        &mut self,
        amount: U512,
        existing_purse: URef,
    ) -> Result<(), Error> {
        if self.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        match <Self as Mint>::mint_into_existing_purse(self, existing_purse, amount) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::MintError)
            }
        }
    }

    fn create_purse(&mut self) -> Result<URef, Error> {
        let initial_balance = U512::zero();
        match <Self as Mint>::mint(self, initial_balance) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::CreatePurseFailed)
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

    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        match <Self as Mint>::read_base_round_reward(self) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::MissingValue)
            }
        }
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        match <Self as Mint>::mint(self, amount) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::MintReward)
            }
        }
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        match <Self as Mint>::reduce_total_supply(self, amount) {
            Ok(ret) => Ok(ret),
            Err(err) => {
                error!("{}", err);
                Err(Error::MintReduceTotalSupply)
            }
        }
    }
}

impl<S> AccountProvider for RuntimeNative<S>
where
    S: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_main_purse(&self) -> Result<URef, Error> {
        // NOTE: this is used by the system and is not (and should not be made to be) accessible
        // from userland.
        match self.runtime_footprint().main_purse() {
            None => {
                debug!("runtime_native attempt to access non-existent main purse");
                Err(Error::InvalidContext)
            }
            Some(purse) => Ok(purse),
        }
    }

    /// Set main purse.
    fn set_main_purse(&mut self, purse: URef) {
        self.runtime_footprint_mut().set_main_purse(purse);
    }
}

impl<S> Auction for RuntimeNative<S> where S: StateReader<Key, StoredValue, Error = GlobalStateError>
{}
