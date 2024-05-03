#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{
    runtime_args,
    system::auction::{ARG_NEW_PUBLIC_KEY, ARG_PUBLIC_KEY, METHOD_CHANGE_BID_PUBLIC_KEY},
    PublicKey,
};

fn change_bid_public_key(public_key: PublicKey, new_public_key: PublicKey) {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        ARG_PUBLIC_KEY => public_key,
        ARG_NEW_PUBLIC_KEY => new_public_key
    };
    runtime::call_contract::<()>(contract_hash, METHOD_CHANGE_BID_PUBLIC_KEY, args);
}

// Change validator bid public key.
//
// Accepts current bid's public key and new public key.
// Updates existing validator bid and all related delegator bids with
// the new public key.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let new_public_key = runtime::get_named_arg(ARG_NEW_PUBLIC_KEY);
    change_bid_public_key(public_key, new_public_key);
}
