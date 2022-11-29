#![no_std]
#![no_main]

extern crate alloc;

use alloc::{collections::BTreeMap, string::String};

use casper_contract::contract_api::{runtime, system};

use casper_types::{
    runtime_args,
    system::auction::{
        ARG_DELEGATOR, ARG_ERA_END_TIMESTAMP_MILLIS, ARG_VALIDATOR, METHOD_DELEGATE,
        METHOD_DISTRIBUTE, METHOD_RUN_AUCTION, METHOD_UNDELEGATE,
    },
    ApiError, PublicKey, RuntimeArgs, U512,
};

const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATE: &str = "delegate";
const ARG_UNDELEGATE: &str = "undelegate";
const ARG_RUN_AUCTION: &str = "run_auction";

#[repr(u16)]
enum Error {
    UnknownCommand,
}

#[no_mangle]
pub extern "C" fn call() {
    let command: String = runtime::get_named_arg(ARG_ENTRY_POINT);

    match command.as_str() {
        ARG_DELEGATE => {
            delegate();
        }
        ARG_UNDELEGATE => {
            undelegate();
        }
        ARG_RUN_AUCTION => run_auction(),
        METHOD_DISTRIBUTE => distribute(),
        _ => runtime::revert(ApiError::User(Error::UnknownCommand as u16)),
    };
}

fn delegate() -> U512 {
    let auction = system::get_auction();
    let delegator: PublicKey = runtime::get_named_arg(ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let args = runtime_args! {
        ARG_DELEGATOR => delegator,
        ARG_VALIDATOR => validator,
        ARG_AMOUNT => amount,
    };

    runtime::call_contract(auction, METHOD_DELEGATE, args)
}

fn undelegate() -> U512 {
    let auction = system::get_auction();
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let delegator: PublicKey = runtime::get_named_arg(ARG_DELEGATOR);
    let validator: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);

    let args = runtime_args! {
        ARG_AMOUNT => amount,
        ARG_VALIDATOR => validator,
        ARG_DELEGATOR => delegator,
    };

    runtime::call_contract(auction, METHOD_UNDELEGATE, args)
}

fn run_auction() {
    let auction = system::get_auction();
    let era_end_timestamp_millis: u64 = runtime::get_named_arg(ARG_ERA_END_TIMESTAMP_MILLIS);
    let args = runtime_args! { ARG_ERA_END_TIMESTAMP_MILLIS => era_end_timestamp_millis };
    runtime::call_contract::<()>(auction, METHOD_RUN_AUCTION, args);
}

fn distribute() {
    let auction = system::get_auction();
    let proposer: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);
    let args = runtime_args! {
        ARG_VALIDATOR => proposer
    };
    runtime::call_contract::<()>(auction, METHOD_DISTRIBUTE, args);
}
