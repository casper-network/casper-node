#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{URef, U512};

const ARG_TARGET_PURSE: &str = "target_purse";
const ARG_AMOUNT: &str = "amount";

#[no_mangle]
pub extern "C" fn call() {
    let target_purse: URef = runtime::get_named_arg(ARG_TARGET_PURSE);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let source_purse = account::get_main_purse();

    system::transfer_from_purse_to_purse(source_purse, target_purse, amount, None)
        .unwrap_or_revert();
}
