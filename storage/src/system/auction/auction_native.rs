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
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    crypto,
    system::{
        auction::{BidAddr, BidKind, EraInfo, Error, UnbondingPurse},
        mint,
    },
    CLTyped, CLValue, HoldsEpoch, Key, KeyTag, PublicKey, StoredValue, URef, U512,
};
use std::collections::BTreeSet;
use tracing::error;

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
        let prefix = bid_addr.delegators_prefix()?;
        let keys = self
            .tracking_copy()
            .borrow_mut()
            .reader()
            .keys_with_prefix(&prefix)
            .map_err(|err| {
                error!("RuntimeProvider::delegator_count {:?}", err);
                Error::Storage
            })?;
        Ok(keys.len())
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

    fn read_unbonds(&mut self, account_hash: &AccountHash) -> Result<Vec<UnbondingPurse>, Error> {
        match self
            .tracking_copy()
            .borrow_mut()
            .read(&Key::Unbond(*account_hash))
        {
            Ok(Some(StoredValue::Unbonding(unbonding_purses))) => Ok(unbonding_purses),
            Ok(Some(_)) => {
                error!("StorageProvider::read_unbonds: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(Vec::new()),
            Err(TrackingCopyError::BytesRepr(_)) => Err(Error::Serialization),
            Err(err) => {
                error!("StorageProvider::read_unbonds: {:?}", err);
                Err(Error::Storage)
            }
        }
    }

    fn write_unbonds(
        &mut self,
        account_hash: AccountHash,
        unbonding_purses: Vec<UnbondingPurse>,
    ) -> Result<(), Error> {
        let unbond_key = Key::Unbond(account_hash);
        if unbonding_purses.is_empty() {
            self.tracking_copy().borrow_mut().prune(unbond_key);
            Ok(())
        } else {
            self.tracking_copy()
                .borrow_mut()
                .write(unbond_key, StoredValue::Unbonding(unbonding_purses));
            Ok(())
        }
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
    fn unbond(&mut self, unbonding_purse: &UnbondingPurse) -> Result<(), Error> {
        let account_hash =
            AccountHash::from_public_key(unbonding_purse.unbonder_public_key(), crypto::blake2b);
        let maybe_value = self
            .tracking_copy()
            .borrow_mut()
            .read(&Key::Account(account_hash))
            .map_err(|error| {
                error!("MintProvider::unbond: {:?}", error);
                Error::Storage
            })?;

        let contract_key: Key = match maybe_value {
            Some(StoredValue::CLValue(cl_value)) => {
                let contract_key: Key = cl_value.into_t().map_err(|_| Error::CLValue)?;
                contract_key
            }
            Some(_cl_value) => return Err(Error::CLValue),
            None => return Err(Error::InvalidPublicKey),
        };

        let maybe_value = self
            .tracking_copy()
            .borrow_mut()
            .read(&contract_key)
            .map_err(|error| {
                error!("MintProvider::unbond: {:?}", error);
                Error::Storage
            })?;

        match maybe_value {
            Some(StoredValue::AddressableEntity(contract)) => {
                self.mint_transfer_direct(
                    Some(account_hash),
                    *unbonding_purse.bonding_purse(),
                    contract.main_purse(),
                    *unbonding_purse.amount(),
                    None,
                    HoldsEpoch::NOT_APPLICABLE, // unbonding purses do not have holds on them
                )
                .map_err(|_| Error::Transfer)?
                .map_err(|_| Error::Transfer)?;
                Ok(())
            }
            Some(_cl_value) => Err(Error::CLValue),
            None => Err(Error::InvalidPublicKey),
        }
    }

    fn mint_transfer_direct(
        &mut self,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
        holds_epoch: HoldsEpoch,
    ) -> Result<Result<(), mint::Error>, Error> {
        if !(self.addressable_entity().main_purse().addr() == source.addr()
            || self.get_caller() == PublicKey::System.to_account_hash())
        {
            return Err(Error::InvalidCaller);
        }

        // let gas_counter = self.gas_counter();
        self.extend_access_rights(&[source, target.into_add()]);

        match self.transfer(to, source, target, amount, id, holds_epoch) {
            Ok(ret) => {
                // self.set_gas_counter(gas_counter);
                Ok(Ok(ret))
            }
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

        // let gas_counter = self.gas_counter();
        match <Self as Mint>::mint_into_existing_purse(self, existing_purse, amount) {
            Ok(ret) => {
                // self.set_gas_counter(gas_counter);
                Ok(ret)
            }
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

    fn available_balance(
        &mut self,
        purse: URef,
        holds_epoch: HoldsEpoch,
    ) -> Result<Option<U512>, Error> {
        match <Self as Mint>::balance(self, purse, holds_epoch) {
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
        Ok(self.addressable_entity().main_purse())
    }
}

impl<S> Auction for RuntimeNative<S> where S: StateReader<Key, StoredValue, Error = GlobalStateError>
{}
