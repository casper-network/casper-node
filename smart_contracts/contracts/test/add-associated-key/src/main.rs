#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::account::{AccountHash, Weight};

const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";

#[no_mangle]
pub extern "C" fn call() {
    let account: AccountHash = runtime::get_named_arg(ARG_ACCOUNT);
    let weight: Weight = runtime::get_named_arg(ARG_WEIGHT);
    account::add_associated_key(account, weight).unwrap_or_revert();
}
