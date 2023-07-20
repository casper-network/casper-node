#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{self, account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, URef, U512};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT_PART_1: &str = "amount_part_1";
const ARG_AMOUNT_PART_2: &str = "amount_part_2";

#[no_mangle]
pub extern "C" fn call() {
    let source_purse: URef = account::get_main_purse();
    let amount_part_1: U512 = runtime::get_named_arg(ARG_AMOUNT_PART_1);
    let amount_part_2: U512 = runtime::get_named_arg(ARG_AMOUNT_PART_2);
    let target_account: AccountHash = runtime::get_named_arg(ARG_TARGET);

    contract_api::system::transfer_from_purse_to_account(
        source_purse,
        target_account,
        amount_part_1,
        None,
    )
    .unwrap_or_revert();

    contract_api::system::transfer_from_purse_to_account(
        source_purse,
        target_account,
        amount_part_2,
        None,
    )
    .unwrap_or_revert();
}
