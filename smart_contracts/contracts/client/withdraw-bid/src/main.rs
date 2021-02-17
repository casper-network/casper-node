#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{runtime_args, system::auction, PublicKey, RuntimeArgs, U512};

const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_AMOUNT: &str = "amount";

fn withdraw_bid(public_key: PublicKey, unbond_amount: U512) -> U512 {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_AMOUNT => unbond_amount,
        auction::ARG_PUBLIC_KEY => public_key,
    };
    runtime::call_contract(contract_hash, auction::METHOD_WITHDRAW_BID, args)
}

// Withdraw bid contract.
//
// Accepts a public key to be removed, and an amount to withdraw (of type `U512`).
// Saves the withdrawn funds in the account's context to keep track of the funds.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    withdraw_bid(public_key, amount);
}
