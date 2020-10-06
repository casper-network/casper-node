#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";

const ARG_VALIDATOR: &str = "validator";
const ARG_DELEGATOR: &str = "delegator";

fn delegate(delegator: PublicKey, validator: PublicKey, source_purse: URef, amount: U512) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator,
        auction::ARG_VALIDATOR => validator,
        auction::ARG_SOURCE_PURSE => source_purse,
        auction::ARG_AMOUNT => amount,
    };
    runtime::call_contract::<(URef, U512)>(contract_hash, auction::METHOD_DELEGATE, args);
}

// Delegate contract.
//
// Accepts a delegator's public key, validator's public key, amount and a delgation rate.
// Issues an delegation request to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let delegator = runtime::get_named_arg(ARG_DELEGATOR);
    let validator = runtime::get_named_arg(ARG_VALIDATOR);
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    // provision bonding purse
    let delegating_purse = {
        let source_purse = account::get_main_purse();
        let bonding_purse = system::create_purse();
        // transfer amount to a bidding purse
        system::transfer_from_purse_to_purse(source_purse, bonding_purse, amount)
            .unwrap_or_revert();
        bonding_purse
    };

    delegate(delegator, validator, delegating_purse, amount);
}
