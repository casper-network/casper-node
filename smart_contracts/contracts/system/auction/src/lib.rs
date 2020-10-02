#![no_std]

#[macro_use]
extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::result::Result as StdResult;

use casper_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    auction::{
        Auction, DelegationRate, MintProvider, RuntimeProvider, SeigniorageRecipients,
        StorageProvider, SystemProvider, ValidatorWeights, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_DELEGATOR, ARG_DELEGATOR_PUBLIC_KEY, ARG_ERA_ID, ARG_PUBLIC_KEY, ARG_REWARD_FACTORS,
        ARG_SOURCE_PURSE, ARG_TARGET_PURSE, ARG_VALIDATOR, ARG_VALIDATOR_KEYS,
        ARG_VALIDATOR_PUBLIC_KEY, ARG_VALIDATOR_PUBLIC_KEYS, METHOD_ADD_BID, METHOD_DELEGATE,
        METHOD_DISTRIBUTE, METHOD_GET_ERA_VALIDATORS, METHOD_QUASH_BID, METHOD_READ_ERA_ID,
        METHOD_READ_SEIGNIORAGE_RECIPIENTS, METHOD_RUN_AUCTION, METHOD_SLASH, METHOD_UNDELEGATE,
        METHOD_WITHDRAW_BID, METHOD_WITHDRAW_DELEGATOR_REWARD, METHOD_WITHDRAW_VALIDATOR_REWARD,
    },
    bytesrepr::{FromBytes, ToBytes},
    mint::{METHOD_MINT, METHOD_READ_BASE_ROUND_REWARD},
    system_contract_errors,
    system_contract_errors::auction::Error,
    CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, PublicKey, RuntimeArgs, TransferResult, URef, BLAKE2B_DIGEST_LENGTH, U512,
};

struct AuctionContract;

impl StorageProvider for AuctionContract {
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error> {
        Ok(storage::read(uref)?)
    }

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error> {
        storage::write(uref, value);
        Ok(())
    }
}

impl SystemProvider for AuctionContract {
    fn create_purse(&mut self) -> URef {
        system::create_purse()
    }

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error> {
        Ok(system::get_balance(purse))
    }

    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> StdResult<(), Error> {
        system::transfer_from_purse_to_purse(source, target, amount).map_err(|_| Error::Transfer)
    }
}

impl RuntimeProvider for AuctionContract {
    fn get_caller(&self) -> AccountHash {
        runtime::get_caller()
    }

    fn get_key(&self, name: &str) -> Option<Key> {
        runtime::get_key(name)
    }

    fn put_key(&mut self, name: &str, key: Key) {
        runtime::put_key(name, key)
    }

    fn blake2b<T: AsRef<[u8]>>(&self, data: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        runtime::blake2b(data)
    }
}

impl MintProvider for AuctionContract {
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> TransferResult {
        system::transfer_from_purse_to_account(source, target, amount)
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ()> {
        system::transfer_from_purse_to_purse(source, target, amount).map_err(|_| ())
    }

    fn balance(&mut self, purse: URef) -> Option<U512> {
        system::get_balance(purse)
    }

    fn read_base_round_reward(&mut self) -> Result<U512, Error> {
        let mint_contract = system::get_mint();
        let runtime_args = RuntimeArgs::default();
        let result: Result<U512, system_contract_errors::mint::Error> =
            runtime::call_contract(mint_contract, METHOD_READ_BASE_ROUND_REWARD, runtime_args);
        result.map_err(|_| Error::MissingValue)
    }

    fn mint(&mut self, amount: U512) -> Result<URef, Error> {
        let mint_contract = system::get_mint();
        let runtime_args = {
            let mut tmp = RuntimeArgs::new();
            tmp.insert(ARG_AMOUNT, amount);
            tmp
        };
        let result: Result<URef, system_contract_errors::mint::Error> =
            runtime::call_contract(mint_contract, METHOD_MINT, runtime_args);
        result.map_err(|_| Error::MintReward)
    }
}

impl Auction for AuctionContract {}

#[no_mangle]
pub extern "C" fn get_era_validators() {
    let era_id = runtime::get_named_arg(ARG_ERA_ID);

    let result = AuctionContract
        .get_era_validators(era_id)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn read_seigniorage_recipients() {
    let result = AuctionContract
        .read_seigniorage_recipients()
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn add_bid() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let source_purse = runtime::get_named_arg(ARG_SOURCE_PURSE);
    let delegation_rate = runtime::get_named_arg(ARG_DELEGATION_RATE);
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .add_bid(public_key, source_purse, delegation_rate, amount)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();

    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn withdraw_bid() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .withdraw_bid(public_key, amount)
        .unwrap_or_revert();
    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn delegate() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let source_purse = runtime::get_named_arg(ARG_SOURCE_PURSE);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .delegate(delegator, source_purse, validator, amount)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn undelegate() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .undelegate(delegator, validator, amount)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn quash_bid() {
    let validator_keys: Vec<PublicKey> = runtime::get_named_arg("validator_keys");

    AuctionContract.quash_bid(validator_keys).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn run_auction() {
    AuctionContract.run_auction().unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn read_era_id() {
    let result = AuctionContract.read_era_id().unwrap_or_revert();
    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value);
}

#[no_mangle]
pub extern "C" fn slash() {
    let validator_public_keys = runtime::get_named_arg(ARG_VALIDATOR_PUBLIC_KEYS);
    AuctionContract
        .slash(validator_public_keys)
        .unwrap_or_revert();
}

#[no_mangle]
pub fn distribute() {
    let reward_factors: BTreeMap<PublicKey, u64> = runtime::get_named_arg(ARG_REWARD_FACTORS);

    AuctionContract
        .distribute(reward_factors)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(()).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub fn withdraw_delegator_reward() {
    let validator_public_key: PublicKey = runtime::get_named_arg(ARG_VALIDATOR_PUBLIC_KEY);
    let delegator_public_key: PublicKey = runtime::get_named_arg(ARG_DELEGATOR_PUBLIC_KEY);
    let target_purse: URef = runtime::get_named_arg(ARG_TARGET_PURSE);

    AuctionContract
        .withdraw_delegator_reward(validator_public_key, delegator_public_key, target_purse)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(()).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub fn withdraw_validator_reward() {
    let validator_public_key: PublicKey = runtime::get_named_arg(ARG_VALIDATOR_PUBLIC_KEY);
    let target_purse: URef = runtime::get_named_arg(ARG_TARGET_PURSE);

    AuctionContract
        .withdraw_validator_reward(validator_public_key, target_purse)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(()).unwrap_or_revert();
    runtime::ret(cl_value)
}

pub fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_GET_ERA_VALIDATORS,
        vec![],
        Option::<ValidatorWeights>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_READ_SEIGNIORAGE_RECIPIENTS,
        vec![],
        SeigniorageRecipients::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_ADD_BID,
        vec![
            Parameter::new(ARG_PUBLIC_KEY, AccountHash::cl_type()),
            Parameter::new(ARG_SOURCE_PURSE, URef::cl_type()),
            Parameter::new(ARG_DELEGATION_RATE, DelegationRate::cl_type()),
            Parameter::new(ARG_AMOUNT, U512::cl_type()),
        ],
        <(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_WITHDRAW_BID,
        vec![
            Parameter::new(ARG_PUBLIC_KEY, AccountHash::cl_type()),
            Parameter::new(ARG_AMOUNT, U512::cl_type()),
        ],
        <(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_DELEGATE,
        vec![
            Parameter::new(ARG_DELEGATOR, PublicKey::cl_type()),
            Parameter::new(ARG_SOURCE_PURSE, URef::cl_type()),
            Parameter::new(ARG_VALIDATOR, PublicKey::cl_type()),
            Parameter::new(ARG_AMOUNT, U512::cl_type()),
        ],
        <(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_UNDELEGATE,
        vec![
            Parameter::new(ARG_DELEGATOR, AccountHash::cl_type()),
            Parameter::new(ARG_VALIDATOR, AccountHash::cl_type()),
            Parameter::new(ARG_AMOUNT, U512::cl_type()),
        ],
        <(URef, U512)>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_QUASH_BID,
        vec![Parameter::new(
            ARG_VALIDATOR_KEYS,
            Vec::<AccountHash>::cl_type(),
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_RUN_AUCTION,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_SLASH,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_DISTRIBUTE,
        vec![Parameter::new(
            ARG_REWARD_FACTORS,
            CLType::Map {
                key: Box::new(CLType::PublicKey),
                value: Box::new(CLType::U64),
            },
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_WITHDRAW_DELEGATOR_REWARD,
        vec![
            Parameter::new(ARG_VALIDATOR_PUBLIC_KEY, CLType::PublicKey),
            Parameter::new(ARG_DELEGATOR_PUBLIC_KEY, CLType::PublicKey),
            Parameter::new(ARG_TARGET_PURSE, CLType::URef),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_WITHDRAW_VALIDATOR_REWARD,
        vec![
            Parameter::new(ARG_VALIDATOR_PUBLIC_KEY, CLType::PublicKey),
            Parameter::new(ARG_TARGET_PURSE, CLType::URef),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_READ_ERA_ID,
        vec![],
        CLType::U64,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
