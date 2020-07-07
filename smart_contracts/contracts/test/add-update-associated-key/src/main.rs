#![no_std]
#![no_main]

use contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use types::account::{AccountHash, Weight};

const INIT_WEIGHT: u8 = 1;
const MOD_WEIGHT: u8 = 2;

const ARG_ACCOUNT: &str = "account";

#[no_mangle]
pub extern "C" fn call() {
    let account: AccountHash = runtime::get_named_arg(ARG_ACCOUNT);

    let weight1 = Weight::new(INIT_WEIGHT);
    account::add_associated_key(account, weight1).unwrap_or_revert();

    let weight2 = Weight::new(MOD_WEIGHT);
    account::update_associated_key(account, weight2).unwrap_or_revert();
}
