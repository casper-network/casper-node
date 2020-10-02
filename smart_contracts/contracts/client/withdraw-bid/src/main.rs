#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef, U512};

const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_AMOUNT: &str = "amount";
const WITHDRAW_BID_PURSE: &str = "withdraw_bid_purse";

fn withdraw_bid(public_key: PublicKey, unbond_amount: U512) -> URef {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_AMOUNT => unbond_amount,
        auction::ARG_PUBLIC_KEY => public_key,
    };
    let (withdraw_bid_purse, _amount) =
        runtime::call_contract::<(URef, U512)>(contract_hash, auction::METHOD_WITHDRAW_BID, args);
    withdraw_bid_purse
}

// Withdraw bid contract.
//
// Accepts a public key to be removed, and an amount to withdraw (of type `U512`).
// Saves the withdrawn funds in the account's context to keep track of the funds.
#[no_mangle]
pub extern "C" fn call() {
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    let amount = runtime::get_named_arg(ARG_AMOUNT);
    let withdraw_bid_purse = withdraw_bid(public_key, amount);

    // Save the purse under account's coontext to keep track of the funds.
    runtime::put_key(WITHDRAW_BID_PURSE, withdraw_bid_purse.into());
}
