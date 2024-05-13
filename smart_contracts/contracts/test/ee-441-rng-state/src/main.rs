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
    EntryPointPayment, EntryPointType, EntryPoints, Key, RuntimeArgs, URef, U512,
};

const ARG_FLAG: &str = "flag";

#[no_mangle]
pub extern "C" fn do_nothing() {
    // Doesn't advance RNG of the runtime
    runtime::ret(CLValue::from_t("Hello, world!").unwrap_or_revert())
}

#[no_mangle]
pub extern "C" fn do_something() {
    // Advances RNG of the runtime
    let test_string = String::from("Hello, world!");

    let test_uref: URef = storage::new_uref(test_string);
    let return_value = CLValue::from_t(test_uref).unwrap_or_revert();
    runtime::ret(return_value)
}

#[no_mangle]
pub extern "C" fn call() {
    let flag: String = runtime::get_named_arg(ARG_FLAG);

    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let do_nothing_entry_point = EntryPoint::new(
            "do_nothing",
            Parameters::default(),
            CLType::String,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );

        entry_points.add_entry_point(do_nothing_entry_point);

        let do_something_entry_point = EntryPoint::new(
            "do_something",
            Parameters::default(),
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        );

        entry_points.add_entry_point(do_something_entry_point);

        entry_points
    };
    let (contract_hash, _contract_version) =
        storage::new_contract(entry_points, None, None, None, None);

    if flag == "pass1" {
        // Two calls should forward the internal RNG. This pass is a baseline.
        let uref1: URef = storage::new_uref(U512::from(0));
        let uref2: URef = storage::new_uref(U512::from(1));
        runtime::put_key("uref1", Key::URef(uref1));
        runtime::put_key("uref2", Key::URef(uref2));
    } else if flag == "pass2" {
        let uref1: URef = storage::new_uref(U512::from(0));
        runtime::put_key("uref1", Key::URef(uref1));
        // do_nothing doesn't do anything. It SHOULD not forward the internal RNG.
        let result: String =
            runtime::call_contract(contract_hash, "do_nothing", RuntimeArgs::default());
        assert_eq!(result, "Hello, world!");
        let uref2: URef = storage::new_uref(U512::from(1));
        runtime::put_key("uref2", Key::URef(uref2));
    } else if flag == "pass3" {
        let uref1: URef = storage::new_uref(U512::from(0));
        runtime::put_key("uref1", Key::URef(uref1));
        // do_something returns a new uref, and it should forward the internal RNG.
        let uref2: URef =
            runtime::call_contract(contract_hash, "do_something", RuntimeArgs::default());
        runtime::put_key("uref2", Key::URef(uref2));
    }
}
