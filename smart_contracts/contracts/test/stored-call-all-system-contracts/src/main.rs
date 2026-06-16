#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage, system};
use casper_types::{
    addressable_entity::{EntityEntryPoint, EntryPoints, Parameters},
    CLType, EntryPointAccess, EntryPointPayment, EntryPointType, Key, RuntimeArgs, URef,
};

const ENTRY_POINT_NAME: &str = "call_system";
const HASH_KEY_NAME: &str = "stored_call_handle_payment_hash";
const PACKAGE_KEY_NAME: &str = "stored_call_handle_payment_package";
const ACCESS_KEY_NAME: &str = "stored_call_handle_payment_access";
const ENTRY_POINT_GET_PAYMENT_PURSE: &str = "get_payment_purse";

#[no_mangle]
pub extern "C" fn call_system() {
    let handle_payment = system::get_handle_payment();

    let _: URef = runtime::call_contract(
        handle_payment,
        ENTRY_POINT_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    let _ = system::create_purse();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut eps = EntryPoints::new();
        eps.add_entry_point(EntityEntryPoint::new(
            ENTRY_POINT_NAME,
            Parameters::new(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Called,
            EntryPointPayment::Caller,
        ));
        eps
    };

    let (contract_hash, _) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_KEY_NAME.into()),
        Some(ACCESS_KEY_NAME.into()),
        None,
    );

    runtime::put_key(HASH_KEY_NAME, Key::Hash(contract_hash.value()));
}
