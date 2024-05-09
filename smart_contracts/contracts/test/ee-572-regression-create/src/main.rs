#![no_std]
#![no_main]

use core::convert::Into;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::Parameters, AccessRights, CLType, CLValue, EntryPoint, EntryPointAccess,
    EntryPointPayment, EntryPointType, EntryPoints, Key, URef,
};

const DATA: &str = "data";
const CONTRACT_NAME: &str = "create";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn create() {
    let reference: URef = storage::new_uref(DATA);
    let read_only_reference: URef = URef::new(reference.addr(), AccessRights::READ);
    let return_value = CLValue::from_t(read_only_reference).unwrap_or_revert();
    runtime::ret(return_value)
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            "create",
            Parameters::default(),
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    let (contract_hash, contract_version) =
        storage::new_contract(entry_points, None, None, None, None);
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_NAME, Key::contract_entity_key(contract_hash));
}
