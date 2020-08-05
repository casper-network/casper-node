use alloc::vec::Vec;
use core::result::Result as StdResult;

use casperlabs_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::AccountHash,
    auction::{
        AuctionProvider, MintProvider, ARG_ACCOUNT_HASH, ARG_AMOUNT, ARG_DELEGATION_RATE,
        ARG_PURSE, {StorageProvider, SystemProvider},
    },
    auction::{DelegationRate, SeigniorageRecipients},
    bytesrepr::{FromBytes, ToBytes},
    runtime_args,
    system_contract_errors::auction::Error,
    CLType, CLTyped, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, RuntimeArgs, URef, U512,
};

const METHOD_RELEASE_FOUNDER: &str = "release_founder";
const METHOD_READ_WINNERS: &str = "read_winners";
const METHOD_READ_SEIGNIORAGE_RECIPIENTS: &str = "read_seigniorage_recipients";
const METHOD_ADD_BID: &str = "add_bid";
const METHOD_WITHDRAW_BID: &str = "withdraw_bid";
const METHOD_DELEGATE: &str = "delegate";
const METHOD_UNDELEGATE: &str = "undelegate";
const METHOD_QUASH_BID: &str = "quash_bid";
const METHOD_RUN_AUCTION: &str = "run_auction";

const BOND: &str = "bond";
const UNBOND: &str = "unbond";
const RELEASE_FOUNDER_STAKE: &str = "release_founder_stake";

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

    fn bond(&mut self, amount: U512, purse: URef) -> Result<(URef, U512), Self::Error> {
        let contract_hash = system::get_mint();
        let args = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PURSE => purse,
        };

        Ok(runtime::call_contract(contract_hash, BOND, args))
    }

    fn unbond(&mut self, amount: U512) -> Result<(URef, U512), Self::Error> {
        let contract_hash = system::get_mint();
        let args = runtime_args! {
            ARG_AMOUNT => amount,
        };
        Ok(runtime::call_contract(contract_hash, UNBOND, args))
    }

    fn release_founder_stake(&mut self, account_hash: AccountHash) -> StdResult<bool, Self::Error> {
        let contract_hash = system::get_mint();
        let args = runtime_args! {
            ARG_ACCOUNT_HASH => account_hash,
        };
        let result = runtime::call_contract(contract_hash, RELEASE_FOUNDER_STAKE, args);
        Ok(result)
    }
}

impl AuctionProvider for AuctionContract {}

#[no_mangle]
pub extern "C" fn release_founder() {
    let account_hash = runtime::get_named_arg("account_hash");

    let result = AuctionContract.release_founder(account_hash);

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value);
}

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
    let account_hash = runtime::get_named_arg("account_hash");
    let source_purse = runtime::get_named_arg("source_purse");
    let delegation_rate = runtime::get_named_arg(ARG_DELEGATION_RATE);
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .add_bid(account_hash, source_purse, delegation_rate, quantity)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn withdraw_bid() {
    let account_hash = runtime::get_named_arg("account_hash");
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .withdraw_bid(account_hash, quantity)
        .unwrap_or_revert();
    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn delegate() {
    let delegator_account_hash = runtime::get_named_arg("delegator_account_hash");
    let source_purse = runtime::get_named_arg("source_purse");
    let validator_account_hash = runtime::get_named_arg("validator_account_hash");
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .delegate(
            delegator_account_hash,
            source_purse,
            validator_account_hash,
            quantity,
        )
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn undelegate() {
    let delegator_account_hash = runtime::get_named_arg("delegator_account_hash");
    let validator_account_hash = runtime::get_named_arg("validator_account_hash");
    let quantity = runtime::get_named_arg(ARG_AMOUNT);

    let result = AuctionContract
        .undelegate(delegator_account_hash, validator_account_hash, quantity)
        .unwrap_or_revert();

    let cl_value = CLValue::from_t(result).unwrap_or_revert();
    runtime::ret(cl_value)
}

#[no_mangle]
pub extern "C" fn quash_bid() {
    let validator_keys: Vec<AccountHash> = runtime::get_named_arg("validator_keys");

    AuctionContract
        .quash_bid(&validator_keys)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn run_auction() {
    AuctionContract.run_auction().unwrap_or_revert();
}

pub fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_RELEASE_FOUNDER,
        vec![Parameter::new(ARG_ACCOUNT_HASH, AccountHash::cl_type())],
        CLType::Bool,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

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
            Parameter::new("account_hash", AccountHash::cl_type()),
            Parameter::new("source_purse", URef::cl_type()),
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
            Parameter::new("account_hash", AccountHash::cl_type()),
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
            Parameter::new("delegator_account_hash", AccountHash::cl_type()),
            Parameter::new("source_purse", URef::cl_type()),
            Parameter::new("validator_account_hash", AccountHash::cl_type()),
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
            Parameter::new("delegator_account_hash", AccountHash::cl_type()),
            Parameter::new("validator_account_hash", AccountHash::cl_type()),
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
            "validator_keys",
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

    entry_points
}
