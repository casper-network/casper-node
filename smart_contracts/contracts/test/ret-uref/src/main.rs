#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec,
};
use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    runtime_args, CLType, CLValue, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    Parameter, URef,
};

const ACCESS_UREF: &str = "access_uref";
const PUT_UREF: &str = "put_uref";
const GET_UREF: &str = "get_uref";
const INSERT_UREF: &str = "insert_uref";
const HASH_KEY_NAME: &str = "ret_uref_contract_hash";

#[no_mangle]
pub extern "C" fn put_uref() {
    let access_uref: URef = runtime::get_named_arg(ACCESS_UREF);
    runtime::put_key(ACCESS_UREF, access_uref.into());
}

#[no_mangle]
pub extern "C" fn get_uref() {
    let uref = runtime::get_key(ACCESS_UREF)
        .unwrap_or_revert()
        .into_uref()
        .unwrap_or_revert();
    runtime::ret(CLValue::from_t(uref).unwrap_or_revert())
}

#[no_mangle]
pub extern "C" fn insert_uref() {
    let contract_hash = runtime::get_named_arg("contract_hash");
    let uref_name: String = runtime::get_named_arg("name");
    let access_uref: URef = runtime::call_contract(contract_hash, GET_UREF, runtime_args! {});
    runtime::put_key(&uref_name, access_uref.into());
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let put_uref_entrypoint = EntryPoint::new(
            PUT_UREF.to_string(),
            vec![Parameter::new(ACCESS_UREF, CLType::URef)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(put_uref_entrypoint);
        let get_uref_entrypoint = EntryPoint::new(
            GET_UREF.to_string(),
            vec![],
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(get_uref_entrypoint);
        let insert_uref_entrypoint = EntryPoint::new(
            INSERT_UREF.to_string(),
            vec![Parameter::new("contract_hash", CLType::ByteArray(32))],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(insert_uref_entrypoint);
        entry_points
    };
    let (contract_hash, _) = storage::new_contract(entry_points, None, None, None);
    runtime::put_key(HASH_KEY_NAME, Key::contract_entity_key(contract_hash));
}
