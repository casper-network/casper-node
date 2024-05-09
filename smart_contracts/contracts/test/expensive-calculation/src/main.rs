#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    addressable_entity::Parameters, CLType, EntryPoint, EntryPointAccess, EntryPointPayment,
    EntryPointType, EntryPoints, Key,
};

const ENTRY_FUNCTION_NAME: &str = "calculate";

#[no_mangle]
pub extern "C" fn calculate() -> u64 {
    let large_prime: u64 = 0xffff_fffb;

    let mut result: u64 = 42;
    // calculate 42^4242 mod large_prime
    for _ in 1..4242 {
        result *= 42;
        result %= large_prime;
    }

    result
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
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

    let (contract_hash, contract_version) =
        storage::new_contract(entry_points, None, None, None, None);
    runtime::put_key(
        "contract_version",
        storage::new_uref(contract_version).into(),
    );
    runtime::put_key(
        "expensive-calculation",
        Key::contract_entity_key(contract_hash),
    );
}
