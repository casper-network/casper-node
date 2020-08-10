use alloc::vec::Vec;
use core::result::Result as StdResult;

use casperlabs_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::AccountHash,
    auction::{
        AuctionProvider, EraId, MintProvider, RuntimeProvider, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_DELEGATOR, ARG_PUBLIC_KEY, ARG_PURSE, ARG_SOURCE_PURSE, ARG_VALIDATOR,
        ARG_VALIDATOR_KEYS, METHOD_ADD_BID, METHOD_DELEGATE, METHOD_QUASH_BID, METHOD_READ_ERA_ID,
        METHOD_READ_SEIGNIORAGE_RECIPIENTS, METHOD_READ_WINNERS, METHOD_RUN_AUCTION,
        METHOD_UNDELEGATE, METHOD_WITHDRAW_BID, {StorageProvider, SystemProvider},
    },
    auction::{CommissionRate, SeigniorageRecipients},
    bytesrepr::{FromBytes, ToBytes},
    runtime_args,
    system_contract_errors::auction::Error,
    CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, PublicKey, RuntimeArgs, URef, U512,
};

const BOND: &str = "bond";
const UNBOND: &str = "unbond";

struct AuctionContract;

impl StorageProvider for AuctionContract {
    type Error = Error;

    fn get_key(&mut self, name: &str) -> Option<Key> {
        runtime::get_key(name)
    }

    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Self::Error> {
        Ok(storage::read(uref)?)
    }

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Self::Error> {
        storage::write(uref, value);
        Ok(())
    }
}

impl SystemProvider for AuctionContract {
    type Error = Error;
    fn create_purse(&mut self) -> URef {
        system::create_purse()
    }
    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Self::Error> {
        Ok(system::get_balance(purse))
    }
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> StdResult<(), Self::Error> {
        system::transfer_from_purse_to_purse(source, target, amount).map_err(|_| Error::Transfer)
    }
}

impl MintProvider for AuctionContract {
    type Error = Error;

    fn bond(
        &mut self,
        public_key: PublicKey,
        amount: U512,
        purse: URef,
    ) -> Result<(URef, U512), Self::Error> {
        let contract_hash = system::get_mint();
        let args = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PURSE => purse,
            ARG_PUBLIC_KEY => public_key,
        };

        Ok(runtime::call_contract(contract_hash, BOND, args))
    }

    fn unbond(&mut self, public_key: PublicKey, amount: U512) -> Result<(URef, U512), Self::Error> {
        let contract_hash = system::get_mint();
        let args = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PUBLIC_KEY => public_key,
        };
        Ok(runtime::call_contract(contract_hash, UNBOND, args))
    }
}

impl RuntimeProvider for AuctionContract {
    fn get_caller(&self) -> AccountHash {
        runtime::get_caller()
    }
}

impl AuctionProvider for AuctionContract {}

#[no_mangle]
pub extern "C" fn read_winners() {
    let result = AuctionContract.read_winners().unwrap_or_revert();

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
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .withdraw_bid(public_key, quantity)
        .unwrap_or_revert();
    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn delegate() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let source_purse = runtime::get_named_arg(ARG_SOURCE_PURSE);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .delegate(delegator, source_purse, validator, quantity)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn undelegate() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .undelegate(delegator, validator, quantity)
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

pub fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_READ_WINNERS,
        vec![],
        <Vec<AccountHash>>::cl_type(),
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
            Parameter::new(ARG_DELEGATION_RATE, CommissionRate::cl_type()),
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
        U512::cl_type(),
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
        METHOD_READ_ERA_ID,
        vec![],
        EraId::cl_type(),
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

    entry_points
}
