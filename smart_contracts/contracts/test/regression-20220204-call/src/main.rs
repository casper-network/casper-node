#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, AccessRights, AddressableEntityHash};

const ARG_PURSE: &str = "purse";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";
const ARG_ENTRYPOINT: &str = "entrypoint";

#[no_mangle]
pub extern "C" fn call() {
    let new_access_rights: AccessRights = runtime::get_named_arg("new_access_rights");

    let entrypoint: String = runtime::get_named_arg(ARG_ENTRYPOINT);

    let contract_hash_key = runtime::get_key(CONTRACT_HASH_NAME).unwrap_or_revert();

    let contract_hash = contract_hash_key
        .into_entity_hash_addr()
        .map(AddressableEntityHash::new)
        .unwrap_or_revert();

    let main_purse_modified = account::get_main_purse().with_access_rights(new_access_rights);

    runtime::call_contract::<()>(
        contract_hash,
        &entrypoint,
        runtime_args! {
            ARG_PURSE => main_purse_modified,
        },
    );
}
