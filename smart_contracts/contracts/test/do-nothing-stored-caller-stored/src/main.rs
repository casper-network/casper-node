#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    addressable_entity::{EntityEntryPoint, EntryPoints, Parameters},
    runtime_args, CLType, EntryPointAccess, EntryPointPayment, EntryPointType, Key,
};

const ENTRY_FUNCTION_NAME: &str = "call_stored";
const HASH_KEY_NAME: &str = "do_nothing_stored_caller_stored_hash";
const PACKAGE_HASH_KEY_NAME: &str = "do_nothing_caller_stored_package_hash";
const ACCESS_KEY_NAME: &str = "do_nothing_stored_access";
const CONTRACT_VERSION: &str = "contract_version";
const ARG_CONTRACT_ADDR: &str = "contract_addr";

#[no_mangle]
pub extern "C" fn call_stored() {
    let contract_hash: [u8; 32] = runtime::get_named_arg(ARG_CONTRACT_ADDR);
    runtime::call_contract(contract_hash.into(), "delegate", runtime_args! {})
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntityEntryPoint::new(
            ENTRY_FUNCTION_NAME,
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_KEY_NAME.into()),
        Some(ACCESS_KEY_NAME.into()),
        None,
    );

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, Key::Hash(contract_hash.value()));
}
