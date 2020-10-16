#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    contracts::NamedKeys, CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    RuntimeArgs,
};

const DO_NOTHING_NAME: &str = "do_nothing";
const DO_SOMETHING_NAME: &str = "do_something";
const DO_HOST_FUNCTION_CALLS_NAME: &str = "do_host_function_calls";
const HASH_KEY_NAME: &str = "contract_package";
const ACCESS_KEY_NAME: &str = "contract_package_access";
const CONTRACT_KEY_NAME: &str = "contract";
const CALLS_DO_NOTHING_LEVEL1_NAME: &str = "calls_do_nothing_level1";
const CALLS_DO_NOTHING_LEVEL2_NAME: &str = "calls_do_nothing_level2";

#[no_mangle]
pub extern "C" fn do_nothing() {}

fn uses_opcodes() -> Box<u64> {
    let long_bytes = DO_NOTHING_NAME
        .chars()
        .chain(DO_SOMETHING_NAME.chars())
        .chain(DO_HOST_FUNCTION_CALLS_NAME.chars())
        .chain(HASH_KEY_NAME.chars())
        .chain(ACCESS_KEY_NAME.chars())
        .chain(CONTRACT_KEY_NAME.chars());

    // Exercises various opcodes. Should cost more than "do_nothing".
    let mut amount = Box::new(1);
    for (i, c) in long_bytes.enumerate() {
        *amount += c as u64;
        *amount *= c as u64;
        *amount ^= i as u64;
        *amount |= i as u64;
        *amount &= i as u64;
    }

    amount
}

#[no_mangle]
pub extern "C" fn do_something() {
    uses_opcodes();
}

#[no_mangle]
pub extern "C" fn do_host_function_calls() {
    // Exercises various host functions. Should cost more than "do_something" and "do_nothing".
    let _blocktime = runtime::get_blocktime();
    let _phase = runtime::get_phase();
    let _caller = runtime::get_caller();
    let _named_keys = runtime::list_named_keys();
}

#[no_mangle]
pub extern "C" fn calls_do_nothing_level1() {
    let contract_package_hash = runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have key");
    runtime::call_versioned_contract(
        contract_package_hash,
        None,
        DO_NOTHING_NAME,
        RuntimeArgs::default(),
    )
}

#[no_mangle]
pub extern "C" fn calls_do_nothing_level2() {
    let contract_package_hash = runtime::get_key(HASH_KEY_NAME)
        .and_then(Key::into_hash)
        .expect("should have key");
    runtime::call_versioned_contract(
        contract_package_hash,
        None,
        CALLS_DO_NOTHING_LEVEL1_NAME,
        RuntimeArgs::default(),
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            DO_NOTHING_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            DO_SOMETHING_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            DO_HOST_FUNCTION_CALLS_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            CALLS_DO_NOTHING_LEVEL1_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        let entry_point = EntryPoint::new(
            CALLS_DO_NOTHING_LEVEL2_NAME,
            Vec::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (contract_package_hash, _access_uref) = storage::create_contract_package_at_hash();

    runtime::put_key(&HASH_KEY_NAME, contract_package_hash.into());

    let mut named_keys = NamedKeys::new();
    named_keys.insert(HASH_KEY_NAME.into(), contract_package_hash.into());

    let (contract_hash, _version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    runtime::put_key(&CONTRACT_KEY_NAME, contract_hash.into());
}
