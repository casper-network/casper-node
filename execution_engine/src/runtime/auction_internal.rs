use std::collections::BTreeSet;
use tracing::{debug, error};

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::{
        auction::{
            providers::{AccountProvider, MintProvider, RuntimeProvider, StorageProvider},
            Auction,
        },
        mint::Mint,
    },
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{
        auction::{BidAddr, BidKind, EraInfo, Error, Unbond, UnbondEra, UnbondKind},
        mint,
    },
    AccessRights, CLTyped, CLValue, Key, KeyTag, PublicKey, RuntimeArgs, StoredValue, URef, U512,
};

use super::Runtime;
use crate::execution::ExecError;

impl From<ExecError> for Option<Error> {
    fn from(exec_error: ExecError) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure [`Auction`]
            // contract running natively supports propagating gas limit errors without a panic.
            ExecError::GasLimit => Some(Error::GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
    }
}

impl<R> StorageProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        match self.context.read_gs(&uref.into()) {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                Ok(Some(cl_value.into_t().map_err(|_| Error::CLValue)?))
            }
            Ok(Some(_)) => {
                error!("StorageProvider::read: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(ExecError::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(ExecError::GasLimit) => Err(Error::GasLimit),
            Err(err) => {
                error!("StorageProvider::read: {:?}", err);
                Err(Error::Storage)
            }
        }
    }

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        let cl_value = CLValue::from_t(value).map_err(|_| Error::CLValue)?;
        self.context
            .metered_write_gs(uref.into(), StoredValue::CLValue(cl_value))
            .map_err(|exec_error| {
                error!("StorageProvider::write: {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })
    }

    fn read_bid(&mut self, key: &Key) -> Result<Option<BidKind>, Error> {
        match self.context.read_gs(key) {
            Ok(Some(StoredValue::BidKind(bid_kind))) => Ok(Some(bid_kind)),
            Ok(Some(_)) => {
                error!("StorageProvider::read_bid: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(ExecError::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(ExecError::GasLimit) => Err(Error::GasLimit),
            Err(err) => {
                error!("StorageProvider::read_bid: {:?}", err);
                Err(Error::Storage)
            }
        }
    }

    fn write_bid(&mut self, key: Key, bid_kind: BidKind) -> Result<(), Error> {
        self.context
            .metered_write_gs_unsafe(key, StoredValue::BidKind(bid_kind))
            .map_err(|exec_error| {
                error!("StorageProvider::write_bid: {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })
    }

    fn read_unbond(&mut self, bid_addr: BidAddr) -> Result<Option<Unbond>, Error> {
        match self.context.read_gs(&Key::BidAddr(bid_addr)) {
            Ok(Some(StoredValue::BidKind(BidKind::Unbond(unbonds)))) => Ok(Some(*unbonds)),
            Ok(Some(_)) => {
                error!("StorageProvider::read_unbonds: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(None),
            Err(ExecError::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(ExecError::GasLimit) => Err(Error::GasLimit),
            Err(err) => {
                error!("StorageProvider::read_unbonds: {:?}", err);
                Err(Error::Storage)
            }
        }
    }

    fn write_unbond(&mut self, bid_addr: BidAddr, unbond: Option<Unbond>) -> Result<(), Error> {
        let unbond_key = Key::BidAddr(bid_addr);
        match unbond {
            Some(unbond) => self
                .context
                .metered_write_gs_unsafe(
                    unbond_key,
                    StoredValue::BidKind(BidKind::Unbond(Box::new(unbond))),
                )
                .map_err(|exec_error| {
                    error!("StorageProvider::write_unbond: {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                }),
            None => {
                self.context.prune_gs_unsafe(unbond_key);
                Ok(())
            }
        }
    }

    fn record_era_info(&mut self, era_info: EraInfo) -> Result<(), Error> {
        Runtime::record_era_info(self, era_info)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::RecordEraInfo))
    }

    fn prune_bid(&mut self, bid_addr: BidAddr) {
        Runtime::prune(self, bid_addr.into());
    }
}

impl<R> RuntimeProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller(&self) -> AccountHash {
        self.context.get_initiator()
    }

    fn is_allowed_session_caller(&self, account_hash: &AccountHash) -> bool {
        Runtime::is_allowed_session_caller(self, account_hash)
    }

    fn is_valid_uref(&self, uref: URef) -> bool {
        self.context.validate_uref(&uref).is_ok()
    }

    fn named_keys_get(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error> {
        self.context.get_keys(key_tag).map_err(|err| {
            error!(%key_tag, "RuntimeProvider::get_keys: {:?}", err);
            Error::Storage
        })
    }

    fn get_keys_by_prefix(&mut self, prefix: &[u8]) -> Result<Vec<Key>, Error> {
        self.context
            .get_keys_with_prefix(prefix)
            .map_err(|exec_error| {
                error!("RuntimeProvider::get_keys_by_prefix: {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })
    }

    fn delegator_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error> {
        let delegated_accounts = {
            let prefix = bid_addr.delegated_account_prefix()?;
            let keys = self
                .context
                .get_keys_with_prefix(&prefix)
                .map_err(|exec_error| {
                    error!("RuntimeProvider::delegator_count accounts {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })?;
            keys.len()
        };
        let delegated_purses = {
            let prefix = bid_addr.delegated_purse_prefix()?;
            let keys = self
                .context
                .get_keys_with_prefix(&prefix)
                .map_err(|exec_error| {
                    error!("RuntimeProvider::delegator_count purses {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })?;
            keys.len()
        };
        Ok(delegated_accounts.saturating_add(delegated_purses))
    }

    fn reservation_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error> {
        let reserved_accounts = {
            let reservation_prefix = bid_addr.reserved_account_prefix()?;
            let reservation_keys = self
                .context
                .get_keys_with_prefix(&reservation_prefix)
                .map_err(|exec_error| {
                    error!("RuntimeProvider::reservation_count {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })?;
            reservation_keys.len()
        };
        let reserved_purses = {
            let reservation_prefix = bid_addr.reserved_purse_prefix()?;
            let reservation_keys = self
                .context
                .get_keys_with_prefix(&reservation_prefix)
                .map_err(|exec_error| {
                    error!("RuntimeProvider::reservation_count {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })?;
            reservation_keys.len()
        };
        Ok(reserved_accounts.saturating_add(reserved_purses))
    }

    fn used_reservation_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error> {
        let reservation_account_prefix = bid_addr.reserved_account_prefix()?;
        let reservation_purse_prefix = bid_addr.reserved_purse_prefix()?;

        let reservation_keys = {
            let mut ret = self
                .context
                .get_keys_with_prefix(&reservation_account_prefix)
                .map_err(|exec_error| {
                    error!("RuntimeProvider::reservation_count {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })?;
            let purses = self
                .context
                .get_keys_with_prefix(&reservation_purse_prefix)
                .map_err(|exec_error| {
                    error!("RuntimeProvider::reservation_count {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })?;
            ret.extend(purses);
            ret
        };

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
                if let Ok(Some(_)) = self.context.read_gs(&key_to_check) {
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
                if let Ok(Some(_)) = self.context.read_gs(&key_to_check) {
                    used += 1;
                }
            }
        }
        Ok(used)
    }

    fn vesting_schedule_period_millis(&self) -> u64 {
        self.context
            .engine_config()
            .vesting_schedule_period_millis()
    }

    fn allow_auction_bids(&self) -> bool {
        self.context.engine_config().allow_auction_bids()
    }

    fn should_compute_rewards(&self) -> bool {
        self.context.engine_config().compute_rewards()
    }
}

impl<R> MintProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn unbond(&mut self, unbond_kind: &UnbondKind, unbond_era: &UnbondEra) -> Result<(), Error> {
        let is_delegator = unbond_kind.is_delegator();
        let (purse, maybe_account_hash) = match unbond_kind {
            UnbondKind::Validator(pk) | UnbondKind::DelegatedPublicKey(pk) => {
                let account_hash = pk.to_account_hash();
                let maybe_value = self
                    .context
                    .read_gs_unsafe(&Key::Account(account_hash))
                    .map_err(|exec_error| {
                        error!("MintProvider::unbond: {:?}", exec_error);
                        <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                    })?;

                match maybe_value {
                    Some(StoredValue::Account(account)) => {
                        (account.main_purse(), Some(account_hash))
                    }
                    Some(StoredValue::CLValue(cl_value)) => {
                        let entity_key: Key = cl_value.into_t().map_err(|_| Error::CLValue)?;
                        match self.context.read_gs_unsafe(&entity_key) {
                            Ok(Some(StoredValue::AddressableEntity(entity))) => {
                                (entity.main_purse(), Some(account_hash))
                            }
                            Ok(Some(StoredValue::CLValue(_))) => {
                                return Err(Error::CLValue);
                            }
                            Ok(Some(_)) => {
                                return if is_delegator {
                                    Err(Error::DelegatorNotFound)
                                } else {
                                    Err(Error::ValidatorNotFound)
                                }
                            }
                            Ok(None) => {
                                return Err(Error::InvalidPublicKey);
                            }
                            Err(exec_error) => {
                                error!("MintProvider::unbond: {:?}", exec_error);
                                return Err(
                                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                                );
                            }
                        }
                    }
                    Some(_) => return Err(Error::UnexpectedStoredValueVariant),
                    None => return Err(Error::InvalidPublicKey),
                }
            }
            UnbondKind::DelegatedPurse(addr) => {
                let purse = URef::new(*addr, AccessRights::READ_ADD_WRITE);
                match self.balance(purse) {
                    Ok(Some(_)) => (purse, None),
                    Ok(None) => return Err(Error::MissingPurse),
                    Err(err) => {
                        error!("MintProvider::unbond delegated purse: {:?}", err);
                        return Err(Error::MintError);
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

    /// Allows optimized auction and mint interaction.
    /// Intended to be used only by system contracts to manage staked purses.
    /// NOTE: Never expose this through FFI.
    fn mint_transfer_direct(
        &mut self,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<Result<(), mint::Error>, Error> {
        let is_main_purse_transfer = self
            .context
            .runtime_footprint()
            .borrow()
            .main_purse()
            .expect("didnt have purse")
            .addr()
            == source.addr();
        let has_perms = is_main_purse_transfer
            || (source.is_writeable() && self.context.validate_uref(&source).is_ok());
        if !(has_perms || self.context.get_initiator() == PublicKey::System.to_account_hash()) {
            return Err(Error::InvalidCaller);
        }

        let args_values = RuntimeArgs::try_new(|args| {
            args.insert(mint::ARG_TO, to)?;
            args.insert(mint::ARG_SOURCE, source)?;
            args.insert(mint::ARG_TARGET, target)?;
            args.insert(mint::ARG_AMOUNT, amount)?;
            args.insert(mint::ARG_ID, id)?;
            Ok(())
        })
        .map_err(|_| Error::CLValue)?;

        let gas_counter = self.gas_counter();

        self.context
            .access_rights_extend(&[source, target.into_add()]);

        let mint_hash = self.get_mint_hash().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;

        let cl_value = self
            .call_contract(mint_hash, mint::METHOD_TRANSFER, args_values)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Transfer))?;

        self.set_gas_counter(gas_counter);
        cl_value.into_t().map_err(|_| Error::CLValue)
    }

    fn mint_into_existing_purse(
        &mut self,
        amount: U512,
        existing_purse: URef,
    ) -> Result<(), Error> {
        if self.context.get_initiator() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let args_values = RuntimeArgs::try_new(|args| {
            args.insert(mint::ARG_AMOUNT, amount)?;
            args.insert(mint::ARG_PURSE, existing_purse)?;
            Ok(())
        })
        .map_err(|_| Error::CLValue)?;

        let gas_counter = self.gas_counter();

        let mint_hash = self.get_mint_hash().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;

        let cl_value = self
            .call_contract(
                mint_hash,
                mint::METHOD_MINT_INTO_EXISTING_PURSE,
                args_values,
            )
            .map_err(|error| <Option<Error>>::from(error).unwrap_or(Error::MintError))?;
        self.set_gas_counter(gas_counter);
        cl_value
            .into_t::<Result<(), mint::Error>>()
            .map_err(|_| Error::CLValue)?
            .map_err(|_| Error::MintError)
    }

    fn create_purse(&mut self) -> Result<URef, Error> {
        Runtime::create_purse(self).map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::CreatePurseFailed)
        })
    }

    fn available_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        Runtime::available_balance(self, purse)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::GetBalance))
    }

    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        let mint_hash = self.get_mint_hash().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;
        self.mint_read_base_round_reward(mint_hash)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        let mint_hash = self.get_mint_hash().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;
        self.mint_mint(mint_hash, amount)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintError))
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        let mint_hash = self.get_mint_hash().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;
        self.mint_reduce_total_supply(mint_hash, amount)
            .map_err(|exec_error| {
                <Option<Error>>::from(exec_error).unwrap_or(Error::MintReduceTotalSupply)
            })
    }
}

impl<R> AccountProvider for Runtime<'_, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_main_purse(&self) -> Result<URef, Error> {
        // NOTE: this is used by the system and is not (and should not be made to be) accessible
        // from userland.
        match Runtime::context(self)
            .runtime_footprint()
            .borrow()
            .main_purse()
        {
            None => {
                debug!("runtime attempt to access non-existent main purse");
                Err(Error::InvalidContext)
            }
            Some(purse) => Ok(purse),
        }
    }

    /// Set main purse.
    fn set_main_purse(&mut self, purse: URef) {
        Runtime::context(self)
            .runtime_footprint()
            .borrow_mut()
            .set_main_purse(purse);
    }
}

impl<R> Auction for Runtime<'_, R> where R: StateReader<Key, StoredValue, Error = GlobalStateError> {}
