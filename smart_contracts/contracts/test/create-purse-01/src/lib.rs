#![no_std]

extern crate alloc;

use alloc::string::String;

use casper_contract::contract_api::{runtime, system};

const ARG_PURSE_NAME: &str = "purse_name";

pub fn delegate() {
    let purse_name: String = runtime::get_named_arg(ARG_PURSE_NAME);
    let purse = system::create_purse();
    runtime::put_key(&purse_name, purse.into());
}
