#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{account, runtime, system};
use casper_types::{auction, runtime_args, PublicKey, RuntimeArgs, URef, U512};

const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_AMOUNT: &str = "amount";
const ARG_UNBOND_PURSE: &str = "unbond_purse";

fn withdraw_bid(public_key: PublicKey, unbond_amount: U512, unbond_purse: URef) -> U512 {
    let contract_hash = system::get_auction();
    let args = runtime_args! {
        auction::ARG_AMOUNT => unbond_amount,
        auction::ARG_PUBLIC_KEY => public_key,
        auction::ARG_UNBOND_PURSE => unbond_purse,
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
    let unbond_purse = {
        let maybe_unbond_purse: Option<URef> = runtime::get_named_arg(ARG_UNBOND_PURSE);
        maybe_unbond_purse.unwrap_or_else(account::get_main_purse)
    };

    withdraw_bid(public_key, amount, unbond_purse);
}
