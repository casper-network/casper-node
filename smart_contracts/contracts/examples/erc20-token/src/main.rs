#![no_std]
// #![cfg_attr(target_arch = "wasm32", no_main)]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_erc20::constants::{
    ARG_ADDRESS, ARG_AMOUNT, ARG_DECIMALS, ARG_NAME, ARG_OWNER, ARG_RECIPIENT, ARG_SPENDER,
    ARG_SYMBOL, ARG_TOTAL_SUPPLY,
};
use casper_types::{account::AccountHash, CLValue, U512};

#[no_mangle]
pub extern "C" fn name() {
    let val = casper_erc20::name();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn symbol() {
    let val = casper_erc20::symbol();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn decimals() {
    let val = casper_erc20::decimals();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn total_supply() {
    let val = casper_erc20::total_supply();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn balance_of() {
    let address: AccountHash = runtime::get_named_arg(ARG_ADDRESS);
    let val = casper_erc20::balance_of(address);
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn transfer() {
    let recipient: AccountHash = runtime::get_named_arg(ARG_RECIPIENT);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    casper_erc20::transfer(&recipient, amount).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn approve() {
    let spender: AccountHash = runtime::get_named_arg(ARG_SPENDER);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    casper_erc20::approve(spender, amount).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn allowance() {
    let owner: AccountHash = runtime::get_named_arg(ARG_OWNER);
    let spender: AccountHash = runtime::get_named_arg(ARG_SPENDER);
    let val = casper_erc20::allowance(owner, spender);
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn transfer_from() {
    let owner: AccountHash = runtime::get_named_arg(ARG_OWNER);
    let recipient: AccountHash = runtime::get_named_arg(ARG_RECIPIENT);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    casper_erc20::transfer_from(owner, recipient, amount).unwrap_or_revert();
}

#[no_mangle]
fn call() {
    let name: String = runtime::get_named_arg(ARG_NAME);
    let symbol: String = runtime::get_named_arg(ARG_SYMBOL);
    let decimals = runtime::get_named_arg(ARG_DECIMALS);
    let total_supply = runtime::get_named_arg(ARG_TOTAL_SUPPLY);

    casper_erc20::delegate(name, symbol, decimals, total_supply).unwrap_or_revert();
}
