#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{account, runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_DELEGATOR: &str = "delegator";
const ARG_VALIDATOR: &str = "validator";
const ARG_UNBOND_PURSE: &str = "unbond_purse";

fn undelegate(delegator: PublicKey, validator: PublicKey, amount: U512, unbond_purse: URef) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_AMOUNT => amount,
        auction::ARG_UNBOND_PURSE => unbond_purse,
    };
    let _amount: U512 = runtime::call_contract(contract_hash, auction::METHOD_UNDELEGATE, args);
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
    let unbond_purse = {
        let maybe_purse: Option<_> = runtime::get_named_arg(ARG_UNBOND_PURSE);
        maybe_purse.unwrap_or_else(account::get_main_purse)
    };
    undelegate(delegator, validator, amount, unbond_purse);
}
