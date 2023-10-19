#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::{NamedKeys, Parameters},
    CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
};

const CONTRACT_PACKAGE_HASH_KEY: &str = "contract_package_hash";
const DISABLED_CONTRACT_HASH_KEY: &str = "disabled_contract_hash";
const ENABLED_CONTRACT_HASH_KEY: &str = "enabled_contract_hash";

#[no_mangle]
pub extern "C" fn do_something() {
    let _ = runtime::list_authorization_keys();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let do_something = EntryPoint::new(
            "do_something",
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );

        entry_points.add_entry_point(do_something);

        entry_points
    };

    let (contract_package_hash, _access_key) = storage::create_contract_package_at_hash();

    let (disabled_contract_hash, _version) = storage::add_contract_version(
        contract_package_hash,
        entry_points.clone(),
        NamedKeys::new(),
    );

    let (enabled_contract_hash, _version) =
        storage::add_contract_version(contract_package_hash, entry_points, NamedKeys::new());

    runtime::put_key(CONTRACT_PACKAGE_HASH_KEY, contract_package_hash.into());

    runtime::put_key(
        DISABLED_CONTRACT_HASH_KEY,
        Key::contract_entity_key(disabled_contract_hash),
    );
    runtime::put_key(
        ENABLED_CONTRACT_HASH_KEY,
        Key::contract_entity_key(enabled_contract_hash),
    );

    storage::disable_contract_version(contract_package_hash, disabled_contract_hash)
        .unwrap_or_revert();
}
