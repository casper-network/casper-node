#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey};

const ARG_VALIDATOR: &str = "validator";

fn activate_bid(public_key: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_VALIDATOR => public_key,
    };
    runtime::call_contract::<()>(contract_hash, auction::METHOD_ACTIVATE_BID, args);
}

// Accepts a public key. Issues an activate-bid bid to the auction contract.
#[no_mangle]
pub extern "C" fn call() {
    let public_key: PublicKey = runtime::get_named_arg(ARG_VALIDATOR);
    activate_bid(public_key);
}
