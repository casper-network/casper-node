#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    addressable_entity::Parameters, CLType, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Key,
};

const CONTRACT_HASH_NAME: &str = "contract";

const NEW_UREF: &str = "new_uref";

// This import below somehow bypasses linkers ability to verify that ext_ffi's new_uref import has
// different signature than the new_uref we're defining in a lib.
#[allow(unused_imports)]
use ee_1071_regression::new_uref;

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            NEW_UREF,
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    let (contract_hash, _contract_version) = storage::new_contract(entry_points, None, None, None);

    runtime::put_key(CONTRACT_HASH_NAME, Key::contract_entity_key(contract_hash));
}
