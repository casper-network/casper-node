#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casperlabs_contract::contract_api::{account, runtime, storage, system};

use casperlabs_types::{
    auction::{
        DelegationRate, SeigniorageRecipients, ARG_DELEGATOR, ARG_PUBLIC_KEY, ARG_SOURCE_PURSE,
        ARG_VALIDATOR, METHOD_ADD_BID, METHOD_DELEGATE, METHOD_READ_SEIGNIORAGE_RECIPIENTS,
        METHOD_RUN_AUCTION, METHOD_UNDELEGATE, METHOD_WITHDRAW_BID,
    },
    runtime_args, ApiError, PublicKey, RuntimeArgs, URef, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ADD_BID: &str = METHOD_ADD_BID;
const ARG_WITHDRAW_BID: &str = METHOD_WITHDRAW_BID;
const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATION_RATE: &str = "delegation_rate";
const ARG_DELEGATE: &str = "delegate";
const ARG_UNDELEGATE: &str = "undelegate";
const ARG_RUN_AUCTION: &str = "run_auction";
const ARG_READ_SEIGNIORAGE_RECIPIENTS: &str = "read_seigniorage_recipients";

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
        ARG_RUN_AUCTION => run_auction(),
        ARG_READ_SEIGNIORAGE_RECIPIENTS => read_seigniorage_recipients(),
        _ => runtime::revert(ApiError::User(Error::UnknownCommand as u16)),
    }
}

fn add_bid() {
    let auction = system::get_auction();
    let public_key: PublicKey = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let delegation_rate: DelegationRate = runtime::get_named_arg(ARG_DELEGATION_RATE);

    let args = runtime_args! {
        ARG_PUBLIC_KEY => public_key,
        ARG_SOURCE_PURSE => account::get_main_purse(),
        ARG_DELEGATION_RATE => delegation_rate,
        ARG_AMOUNT => amount,
    };

    runtime::call_contract::<(URef, U512)>(auction, METHOD_ADD_BID, args);
}

fn withdraw_bid() {
    let auction = system::get_auction();
    let public_key: PublicKey = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let args = runtime_args! {
        ARG_AMOUNT => amount,
        ARG_PUBLIC_KEY => public_key,
    };

    let (_purse, _quantity): (URef, U512) =
        runtime::call_contract(auction, METHOD_WITHDRAW_BID, args);
}

fn delegate() {
    let auction = system::get_auction();
    let delegator: PublicKey = runtime::get_named_arg(ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);
    let quantity: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let args = runtime_args! {
        ARG_DELEGATOR => delegator,
        ARG_VALIDATOR => validator,
        ARG_SOURCE_PURSE => account::get_main_purse(),
        ARG_AMOUNT => quantity,
    };

    let (_purse, _quantity): (URef, U512) = runtime::call_contract(auction, METHOD_DELEGATE, args);
}

fn undelegate() {
    let auction = system::get_auction();
    let quantity: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let delegator: PublicKey = runtime::get_named_arg(ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);

    let args = runtime_args! {
        ARG_AMOUNT => quantity,
        ARG_VALIDATOR => validator,
        ARG_DELEGATOR => delegator,
    };

    let _total_amount: U512 = runtime::call_contract(auction, METHOD_UNDELEGATE, args);
}

fn run_auction() {
    let auction = system::get_auction();
    let args = runtime_args! {};
    runtime::call_contract::<()>(auction, METHOD_RUN_AUCTION, args);
}

fn read_seigniorage_recipients() {
    let auction = system::get_auction();
    let args = runtime_args! {};
    let result: SeigniorageRecipients =
        runtime::call_contract(auction, METHOD_READ_SEIGNIORAGE_RECIPIENTS, args);
    let uref = storage::new_uref(result);
    runtime::put_key("seigniorage_recipients_result", uref.into());
}
