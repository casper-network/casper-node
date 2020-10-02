#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATOR: &str = "delegator";
const ARG_VALIDATOR: &str = "validator";
const UNDELEGATE_PURSE: &str = "undelegate_purse";

fn undelegate(delegator: PublicKey, validator: PublicKey, amount: U512) -> URef {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => amount,
    };
    let (undelegate_purse, _amount) =
        runtime::call_contract::<(URef, U512)>(contract_hash, auction::METHOD_UNDELEGATE, args);
    undelegate_purse
}

// Undelegate contract.
//
// Accepts a delegator's public key, validator's public key to be undelegated, and an amount
// to withdraw (of type `U512`).
#[no_mangle]
pub extern "C" fn call() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    let undelegate_purse = undelegate(delegator, validator, amount);

    // Save the purse under account's coontext to keep track of the funds.
    runtime::put_key(UNDELEGATE_PURSE, undelegate_purse.into());
}
