#![no_std]
#![no_main]

use casper_contract::contract_api::{runtime, storage, system};
use casper_types::{
    addressable_entity::{EntityEntryPoint, EntryPoints, Parameters},
    runtime_args,
    system::mint,
    CLType, EntryPointAccess, EntryPointPayment, EntryPointType, Key, U512,
};

const ENTRY_POINT_NAME: &str = "call_system";
const HASH_KEY_NAME: &str = "stored_call_all_system_contracts_hash";
const PACKAGE_KEY_NAME: &str = "stored_call_all_system_contracts_package";
const ACCESS_KEY_NAME: &str = "stored_call_all_system_contracts_access";

#[no_mangle]
pub extern "C" fn call_system() {
    // A called contract has no access to the caller's main purse, so use a purse it creates.
    let purse = system::create_purse();

    let _: Option<U512> = runtime::call_contract(
        system::get_mint(),
        mint::METHOD_BALANCE,
        runtime_args! { mint::ARG_PURSE => purse },
    );
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
