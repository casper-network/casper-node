#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
};

use casper_contract::{
    contract_api::{account, runtime},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{runtime_args, AccessRights, ContractHash, Key, RuntimeArgs};

const ARG_PURSE: &str = "purse";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";
const ARG_ENTRYPOINT: &str = "entrypoint";

type NonTrivialArg = BTreeMap<String, Key>;

#[no_mangle]
pub extern "C" fn call() {
    let new_access_rights: AccessRights = runtime::get_named_arg("new_access_rights");

    let entrypoint: String = runtime::get_named_arg(ARG_ENTRYPOINT);

    let contract_hash_key = runtime::get_key(CONTRACT_HASH_NAME).unwrap_or_revert();
    let contract_hash = contract_hash_key
        .into_hash()
        .map(ContractHash::new)
        .unwrap_or_revert();

    let main_purse_modified = account::get_main_purse().with_access_rights(new_access_rights);

    let mut nontrivial_arg = NonTrivialArg::new();
    nontrivial_arg.insert("anything".to_string(), Key::from(main_purse_modified));

    runtime::call_contract::<()>(
        contract_hash,
        &entrypoint,
        runtime_args! {
            ARG_PURSE => nontrivial_arg,
        },
    );
}
