#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use casper_contract::contract_api::{runtime, storage};

use casper_types::{
    addressable_entity::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter},
    CLType, Key,
};

const ENTRY_FUNCTION_NAME: &str = "transfer";
const PACKAGE_HASH_KEY_NAME: &str = "transfer_purse_to_account";
const HASH_KEY_NAME: &str = "transfer_purse_to_account_hash";
const ACCESS_KEY_NAME: &str = "transfer_purse_to_account_access";
const ARG_0_NAME: &str = "target_account_addr";
const ARG_1_NAME: &str = "amount";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn transfer() {
    transfer_purse_to_account::delegate();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![
                Parameter::new(ARG_0_NAME, CLType::ByteArray(32)),
                Parameter::new(ARG_1_NAME, CLType::U512),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );
        entry_points.add_entry_point(entry_point);
        entry_points
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, Key::contract_entity_key(contract_hash));
}
