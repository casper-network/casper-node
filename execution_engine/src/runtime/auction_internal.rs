use std::collections::BTreeSet;
use tracing::error;

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    system::auction::{
        providers::{AccountProvider, MintProvider, RuntimeProvider, StorageProvider},
        Auction,
    },
};
use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    crypto,
    system::{
        auction::{BidAddr, BidKind, EraInfo, Error, UnbondingPurse},
        mint,
    },
    CLTyped, CLValue, Key, KeyTag, PublicKey, RuntimeArgs, StoredValue, URef, U512,
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

impl<'a, R> StorageProvider for Runtime<'a, R>
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

    fn read_unbonds(&mut self, account_hash: &AccountHash) -> Result<Vec<UnbondingPurse>, Error> {
        match self.context.read_gs(&Key::Unbond(*account_hash)) {
            Ok(Some(StoredValue::Unbonding(unbonding_purses))) => Ok(unbonding_purses),
            Ok(Some(_)) => {
                error!("StorageProvider::read_unbonds: unexpected StoredValue variant");
                Err(Error::Storage)
            }
            Ok(None) => Ok(Vec::new()),
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

    fn write_unbonds(
        &mut self,
        account_hash: AccountHash,
        unbonding_purses: Vec<UnbondingPurse>,
    ) -> Result<(), Error> {
        let unbond_key = Key::Unbond(account_hash);
        if unbonding_purses.is_empty() {
            self.context.prune_gs_unsafe(unbond_key);
            Ok(())
        } else {
            self.context
                .metered_write_gs_unsafe(unbond_key, StoredValue::Unbonding(unbonding_purses))
                .map_err(|exec_error| {
                    error!("StorageProvider::write_unbonds: {:?}", exec_error);
                    <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
                })
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

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }

    fn is_allowed_session_caller(&self, account_hash: &AccountHash) -> bool {
        Runtime::is_allowed_session_caller(self, account_hash)
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
        let prefix = bid_addr.delegators_prefix()?;
        let keys = self
            .context
            .get_keys_with_prefix(&prefix)
            .map_err(|exec_error| {
                error!("RuntimeProvider::delegator_count {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })?;
        Ok(keys.len())
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

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn unbond(&mut self, unbonding_purse: &UnbondingPurse) -> Result<(), Error> {
        let account_hash =
            AccountHash::from_public_key(unbonding_purse.unbonder_public_key(), crypto::blake2b);
        let maybe_value = self
            .context
            .read_gs_unsafe(&Key::Account(account_hash))
            .map_err(|exec_error| {
                error!("MintProvider::unbond: {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
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
            .context
            .read_gs_unsafe(&contract_key)
            .map_err(|exec_error| {
                error!("MintProvider::unbond: {:?}", exec_error);
                <Option<Error>>::from(exec_error).unwrap_or(Error::Storage)
            })?;

        match maybe_value {
            Some(StoredValue::AddressableEntity(contract)) => {
                self.mint_transfer_direct(
                    Some(account_hash),
                    *unbonding_purse.bonding_purse(),
                    contract.main_purse(),
                    *unbonding_purse.amount(),
                    None,
                )
                .map_err(|_| Error::Transfer)?
                .map_err(|_| Error::Transfer)?;
                Ok(())
            }
            Some(_cl_value) => Err(Error::CLValue),
            None => Err(Error::InvalidPublicKey),
        }
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
        if !(self.context.entity().main_purse().addr() == source.addr()
            || self.context.get_caller() == PublicKey::System.to_account_hash())
        {
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

        let mint_contract_hash = self.get_mint_contract().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;

        let cl_value = self
            .call_contract(mint_contract_hash, mint::METHOD_TRANSFER, args_values)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Transfer))?;

        self.set_gas_counter(gas_counter);
        cl_value.into_t().map_err(|_| Error::CLValue)
    }

    fn mint_into_existing_purse(
        &mut self,
        amount: U512,
        existing_purse: URef,
    ) -> Result<(), Error> {
        if self.context.get_caller() != PublicKey::System.to_account_hash() {
            return Err(Error::InvalidCaller);
        }

        let args_values = RuntimeArgs::try_new(|args| {
            args.insert(mint::ARG_AMOUNT, amount)?;
            args.insert(mint::ARG_PURSE, existing_purse)?;
            Ok(())
        })
        .map_err(|_| Error::CLValue)?;

        let gas_counter = self.gas_counter();

        let mint_contract_hash = self.get_mint_contract().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;

        let cl_value = self
            .call_contract(
                mint_contract_hash,
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
        let mint_contract = self.get_mint_contract().map_err(|exec_error| {
            <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue)
        })?;
        self.mint_read_base_round_reward(mint_contract)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MissingValue))
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        let mint_contract = self
            .get_mint_contract()
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))?;
        self.mint_mint(mint_contract, amount)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))
    }

    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error> {
        let mint_contract = self
            .get_mint_contract()
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))?;
        self.mint_reduce_total_supply(mint_contract, amount)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::MintReward))
    }
}

impl<'a, R> AccountProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    fn get_main_purse(&self) -> Result<URef, Error> {
        // NOTE: this is used by the system and is not (and should not be made to be) accessible
        // from userland.
        Ok(Runtime::context(self).entity().main_purse())
    }
}

impl<'a, R> Auction for Runtime<'a, R> where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>
{
}
