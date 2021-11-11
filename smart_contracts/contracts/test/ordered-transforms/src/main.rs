#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;
use alloc::{string::ToString, vec::Vec};

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    CLType, CLTyped, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, Parameter,
    URef,
};

#[no_mangle]
pub extern "C" fn call() {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        "perform_operations",
        vec![Parameter::new(
            "operations",
            Vec::<(u8, u32, i32)>::cl_type(),
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    let n: u32 = runtime::get_named_arg("n");
    let named_keys = (0..n)
        .map(|i| (format!("uref-{}", i), Key::URef(storage::new_uref(0_i32))))
        .chain(core::iter::once((
            "n-urefs".to_string(),
            Key::URef(storage::new_uref(n)),
        )));

    let (contract_hash, _contract_version) =
        storage::new_locked_contract(entry_points, Some(named_keys.collect()), None, None);
    runtime::put_key("ordered-transforms-contract-hash", contract_hash.into());
}

#[no_mangle]
pub extern "C" fn perform_operations() {
    // List of operations to be performed by the contract.
    // An operation is a tuple (t, i, v) where:
    // * `t` is the operation type: 0 for reading, 1 for writing and 2 for adding;
    // * `i` is the URef index;
    // * `v` is the value to write or add (always zero for reads).
    let operations: Vec<(u8, u32, i32)> = runtime::get_named_arg("operations");
    let n: u32 = storage::read(match runtime::get_key("n-urefs").unwrap_or_revert() {
        Key::URef(uref) => uref,
        _ => panic!("Bad number of URefs."),
    })
    .unwrap_or_revert()
    .unwrap_or_revert();
    let urefs: Vec<URef> = (0..n)
        .map(
            |i| match runtime::get_key(&format!("uref-{}", i)).unwrap_or_revert() {
                Key::URef(uref) => uref,
                _ => panic!("Bad URef."),
            },
        )
        .collect();

    for (t, i, v) in operations {
        let uref = *urefs.get(i as usize).unwrap_or_revert();
        match t {
            0 => {
                let _: Option<i32> = storage::read(uref).unwrap_or_revert();
            }
            1 => storage::write(uref, v),
            2 => storage::add(uref, v),
            _ => panic!("Bad transform type"),
        }
    }
}
