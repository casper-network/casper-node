#![no_std]
// #![cfg_attr(target_arch = "wasm32", no_main)]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_erc20::{
    constants::{
        ARG_ADDRESS, ARG_AMOUNT, ARG_DECIMALS, ARG_NAME, ARG_OWNER, ARG_RECIPIENT, ARG_SPENDER,
        ARG_SYMBOL, ARG_TOTAL_SUPPLY,
    },
    Address, ERC20,
};
use casper_types::{CLValue, U256};

#[no_mangle]
pub extern "C" fn name() {
    let val = ERC20::default().name();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn symbol() {
    let val = ERC20::default().symbol();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn decimals() {
    let val = ERC20::default().decimals();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn total_supply() {
    let val = ERC20::default().total_supply();
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn balance_of() {
    let address: Address = runtime::get_named_arg(ARG_ADDRESS);
    let val = ERC20::default().balance_of(&address);
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn transfer() {
    let recipient: Address = runtime::get_named_arg(ARG_RECIPIENT);
    let amount: U256 = runtime::get_named_arg(ARG_AMOUNT);

    ERC20::default()
        .transfer(&recipient, amount)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn approve() {
    let spender: Address = runtime::get_named_arg(ARG_SPENDER);
    let amount: U256 = runtime::get_named_arg(ARG_AMOUNT);

    ERC20::default()
        .approve(&spender, amount)
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn allowance() {
    let owner: Address = runtime::get_named_arg(ARG_OWNER);
    let spender: Address = runtime::get_named_arg(ARG_SPENDER);
    let val = ERC20::default().allowance(owner, spender);
    runtime::ret(CLValue::from_t(val).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn transfer_from() {
    let owner: Address = runtime::get_named_arg(ARG_OWNER);
    let recipient: Address = runtime::get_named_arg(ARG_RECIPIENT);
    let amount: U256 = runtime::get_named_arg(ARG_AMOUNT);
    ERC20::default()
        .transfer_from(owner, recipient, amount)
        .unwrap_or_revert();
}

#[no_mangle]
fn call() {
    let name: String = runtime::get_named_arg(ARG_NAME);
    let symbol: String = runtime::get_named_arg(ARG_SYMBOL);
    let decimals = runtime::get_named_arg(ARG_DECIMALS);
    let total_supply = runtime::get_named_arg(ARG_TOTAL_SUPPLY);

    let _token = ERC20::install(name, symbol, decimals, total_supply).unwrap_or_revert();
}
