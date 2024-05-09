#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    addressable_entity::Parameters, CLType, CLValue, EntryPoint, EntryPointAccess,
    EntryPointPayment, EntryPointType, EntryPoints, Key, URef,
};

const HELLO_EXT: &str = "hello_ext";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn hello_ext() {
    let test_string = String::from("Hello, world!");
    let test_uref: URef = storage::new_uref(test_string);
    let return_value = CLValue::from_t(test_uref).unwrap_or_revert();
    runtime::ret(return_value)
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            HELLO_EXT,
            Parameters::new(),
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
    runtime::put_key(HELLO_EXT, Key::contract_entity_key(contract_hash));
}
