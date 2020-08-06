#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_contract::contract_api::{account, runtime, system};

use casperlabs_types::{
    account::AccountHash,
    auction::{
        DelegationRate, ARG_ACCOUNT_HASH, ARG_DELEGATOR_ACCOUNT_HASH, ARG_SOURCE_PURSE,
        ARG_VALIDATOR_ACCOUNT_HASH, METHOD_ADD_BID, METHOD_DELEGATE, METHOD_UNDELEGATE,
        METHOD_WITHDRAW_BID,
    },
    runtime_args, ApiError, RuntimeArgs, URef, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ADD_BID: &str = METHOD_ADD_BID;
const ARG_WITHDRAW_BID: &str = METHOD_WITHDRAW_BID;
const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATION_RATE: &str = "delegation_rate";
const ARG_DELEGATE: &str = "delegate";
const ARG_UNDELEGATE: &str = "undelegate";

#[repr(u16)]
enum Error {
    UnknownCommand,
}

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_ENTRY_POINT);

    match command.as_str() {
        ARG_ADD_BID => add_bid(),
        ARG_WITHDRAW_BID => withdraw_bid(),
        ARG_DELEGATE => delegate(),
        ARG_UNDELEGATE => undelegate(),
        _ => runtime::revert(ApiError::User(Error::UnknownCommand as u16)),
    }
}

fn add_bid() {
    let auction = system::get_auction();
    let quantity: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let delegation_rate: DelegationRate = runtime::get_named_arg(ARG_DELEGATION_RATE);

    let args = runtime_args! {
        ARG_ACCOUNT_HASH => runtime::get_caller(),
        ARG_SOURCE_PURSE => account::get_main_purse(),
        ARG_DELEGATION_RATE => delegation_rate,
        ARG_AMOUNT => quantity,
    };

    let (_purse, _quantity): (URef, U512) = runtime::call_contract(auction, METHOD_ADD_BID, args);
}

fn withdraw_bid() {
    let auction = system::get_auction();
    let quantity: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let args = runtime_args! {
        ARG_AMOUNT => quantity,
        ARG_ACCOUNT_HASH => runtime::get_caller(),
    };

    let (_purse, _quantity): (URef, U512) =
        runtime::call_contract(auction, METHOD_WITHDRAW_BID, args);
}

fn delegate() {
    let auction = system::get_auction();
    let validator: AccountHash = runtime::get_named_arg(ARG_VALIDATOR_ACCOUNT_HASH);
    let quantity: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let args = runtime_args! {
        ARG_DELEGATOR_ACCOUNT_HASH => runtime::get_caller(),
        ARG_SOURCE_PURSE => account::get_main_purse(),
        ARG_VALIDATOR_ACCOUNT_HASH => validator,
        ARG_AMOUNT => quantity,
    };

    let (_purse, _quantity): (URef, U512) = runtime::call_contract(auction, METHOD_DELEGATE, args);
}

fn undelegate() {
    let auction = system::get_auction();
    let quantity: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let validator: AccountHash = runtime::get_named_arg(ARG_VALIDATOR_ACCOUNT_HASH);

    let args = runtime_args! {
        ARG_AMOUNT => quantity,
        ARG_ACCOUNT_HASH => runtime::get_caller(),
        ARG_VALIDATOR_ACCOUNT_HASH => validator,
        ARG_DELEGATOR_ACCOUNT_HASH => runtime::get_caller(),
    };

    let _total_amount: U512 = runtime::call_contract(auction, METHOD_UNDELEGATE, args);
}
