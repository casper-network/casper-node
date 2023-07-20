#![no_std]

use casper_contract::{
    contract_api::{account, runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, URef, U512};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

pub fn delegate() {
    let source: URef = account::get_main_purse();
    let target: AccountHash = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    let _transfer_result =
        system::transfer_from_purse_to_account(source, target, amount, None).unwrap_or_revert();
}
