#![no_std]
#![no_main]

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{PublicKey, U512};

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

/// Executes mote transfer to supplied account hash.
/// Transfers the requested amount.
#[no_mangle]
pub extern "C" fn call() {
    let account_hash: PublicKey = runtime::get_named_arg(ARG_TARGET);
    let transfer_amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    system::transfer_to_public_key(account_hash, transfer_amount, None).unwrap_or_revert();
}
