#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{PublicKey, U512};

const ARG_TARGET: &str = "target";
const ARG_SOURCE_PURSE: &str = "source_purse";
const ARG_AMOUNT: &str = "amount";

#[no_mangle]
pub extern "C" fn call() {
    let source_purse = runtime::get_named_arg(ARG_SOURCE_PURSE);
    let target: PublicKey = runtime::get_named_arg(ARG_TARGET);
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);

    system::transfer_from_purse_to_public_key(source_purse, target, amount, None)
        .unwrap_or_revert();
}
