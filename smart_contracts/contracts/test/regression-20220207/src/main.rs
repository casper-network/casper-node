#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{self, account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, URef, U512};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT_TO_SEND: &str = "amount_to_send";

#[no_mangle]
pub extern "C" fn call() {
    let source_purse: URef = account::get_main_purse();
    let amount_to_send: U512 = runtime::get_named_arg(ARG_AMOUNT_TO_SEND);
    let target_account: AccountHash = runtime::get_named_arg(ARG_TARGET);

    contract_api::system::transfer_from_purse_to_account(
        source_purse,
        target_account,
        amount_to_send,
        None,
    )
    .unwrap_or_revert();
}
