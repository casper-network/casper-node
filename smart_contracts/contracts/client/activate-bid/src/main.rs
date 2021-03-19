#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, RuntimeArgs};

const ARG_VALIDATOR_PUBLIC_KEY: &str = "validator_public_key";

fn activate_bid(public_key: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR_PUBLIC_KEY => public_key,
    };
    runtime::call_contract::<()>(contract_hash, auction::METHOD_ACTIVATE_BID, args);
}

// Accepts a public key. Issues an activate-bid bid to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let public_key: PublicKey = runtime::get_named_arg(ARG_VALIDATOR_PUBLIC_KEY);
    activate_bid(public_key);
}
