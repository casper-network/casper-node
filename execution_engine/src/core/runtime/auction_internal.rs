use std::collections::BTreeSet;

use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    crypto,
    system::{
        auction::{Bid, EraInfo, Error, UnbondingPurse},
        mint,
    },
    CLTyped, CLValue, EraId, Key, KeyTag, PublicKey, RuntimeArgs, StoredValue, URef,
    BLAKE2B_DIGEST_LENGTH, U512,
};

use super::Runtime;
use crate::{
    core::execution,
    storage::global_state::StateReader,
    system::auction::{
        providers::{AccountProvider, MintProvider, RuntimeProvider, StorageProvider},
        Auction,
    },
};

impl From<execution::Error> for Option<Error> {
    fn from(exec_error: execution::Error) -> Self {
        match exec_error {
            // This is used to propagate [`execution::Error::GasLimit`] to make sure [`Auction`]
            // contract running natively supports propagating gas limit errors without a panic.
            execution::Error::GasLimit => Some(Error::GasLimit),
            // There are possibly other exec errors happening but such translation would be lossy.
            _ => None,
        }
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
        self.context
            .metered_write_gs(uref.into(), StoredValue::CLValue(cl_value))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn read_bid(&mut self, account_hash: &AccountHash) -> Result<Option<Bid>, Error> {
        match self.context.read_gs(&Key::Bid(*account_hash)) {
            Ok(Some(StoredValue::Bid(bid))) => Ok(Some(*bid)),
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(None),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(execution::Error::GasLimit) => Err(Error::GasLimit),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write_bid(&mut self, account_hash: AccountHash, bid: Bid) -> Result<(), Error> {
        self.context
            .metered_write_gs_unsafe(Key::Bid(account_hash), StoredValue::Bid(Box::new(bid)))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn read_unbond(&mut self, account_hash: &AccountHash) -> Result<Vec<UnbondingPurse>, Error> {
        match self.context.read_gs(&Key::Unbond(*account_hash)) {
            Ok(Some(StoredValue::Unbonding(unbonding_purses))) => Ok(unbonding_purses),
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(Vec::new()),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            // NOTE: This extra condition is needed to correctly propagate GasLimit to the user. See
            // also [`Runtime::reverter`] and [`to_auction_error`]
            Err(execution::Error::GasLimit) => Err(Error::GasLimit),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write_unbond(
        &mut self,
        account_hash: AccountHash,
        unbonding_purses: Vec<UnbondingPurse>,
    ) -> Result<(), Error> {
        self.context
            .metered_write_gs_unsafe(
                Key::Unbond(account_hash),
                StoredValue::Unbonding(unbonding_purses),
            )
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))
    }

    fn record_era_info(&mut self, era_id: EraId, era_info: EraInfo) -> Result<(), Error> {
        Runtime::record_era_info(self, era_id, era_info)
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::RecordEraInfo))
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

    fn is_allowed_session_caller(&self, account_hash: &AccountHash) -> bool {
        Runtime::is_allowed_session_caller(self, account_hash)
    }

    fn named_keys_get(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error> {
        self.context.get_keys(key_tag).map_err(|_| Error::Storage)
    }

    fn blake2b<T: AsRef<[u8]>>(&self, data: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        crypto::blake2b(data)
    }
}

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn unbond(&mut self, unbonding_purse: &UnbondingPurse) -> Result<(), Error> {
        let account_hash =
            AccountHash::from_public_key(unbonding_purse.unbonder_public_key(), crypto::blake2b);
        let maybe_value = self
            .context
            .read_gs_direct(&Key::Account(account_hash))
            .map_err(|exec_error| <Option<Error>>::from(exec_error).unwrap_or(Error::Storage))?;
        match maybe_value {
            Some(StoredValue::Account(account)) => {
                self.mint_transfer_direct(
                    Some(account_hash),
                    *unbonding_purse.bonding_purse(),
                    account.main_purse(),
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
        if !(self.context.account().main_purse().addr() == source.addr()
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

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        Runtime::get_balance(self, purse)
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
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_main_purse(&self) -> Result<URef, Error> {
        // NOTE: This violates security as system contract is a contract entrypoint and normal
        // "get_main_purse" won't work for security reasons. But since we're not running it as a
        // WASM contract, and purses are going to be removed anytime soon, we're making this
        // exception here.
        Ok(Runtime::context(self).account().main_purse())
    }
}

impl<'a, R> Auction for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
