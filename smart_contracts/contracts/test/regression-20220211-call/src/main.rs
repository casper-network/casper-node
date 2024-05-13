#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use casper_contract::{contract_api::runtime, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{AddressableEntityHash, RuntimeArgs, URef};

const CONTRACT_HASH_NAME: &str = "regression-contract-hash";
const ARG_ENTRYPOINT: &str = "entrypoint";

#[no_mangle]
pub extern "C" fn call() {
    let entrypoint: String = runtime::get_named_arg(ARG_ENTRYPOINT);
    let contract_hash_key = runtime::get_key(CONTRACT_HASH_NAME).unwrap_or_revert();
    let contract_hash = contract_hash_key
        .into_entity_hash_addr()
        .map(AddressableEntityHash::new)
        .unwrap_or_revert();

    let hardcoded_uref: URef =
        runtime::call_contract(contract_hash, &entrypoint, RuntimeArgs::default());

    assert!(!runtime::is_valid_uref(hardcoded_uref));
    assert!(!hardcoded_uref.is_writeable(),);
}
