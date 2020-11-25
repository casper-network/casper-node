#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use alloc::boxed::Box;
use casper_contract::contract_api::{runtime, storage};

use casper_types::{
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter},
    CLType,
};

const ENTRY_FUNCTION_NAME: &str = "transfer";

const PACKAGE_HASH_KEY_NAME: &str = "transfer_purse_to_accounts";
const HASH_KEY_NAME: &str = "transfer_purse_to_accounts_hash";
const ACCESS_KEY_NAME: &str = "transfer_purse_to_accounts_access";

const ARG_SOURCE: &str = "source";
const ARG_TARGETS: &str = "targets";

const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn transfer() {
    transfer_purse_to_accounts::delegate();
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut tmp = EntryPoints::new();
        let entry_point = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![
                Parameter::new(ARG_SOURCE, CLType::URef),
                Parameter::new(
                    ARG_TARGETS,
                    CLType::Map {
                        key: Box::new(CLType::ByteArray(32)),
                        value: Box::new(CLType::U512),
                    },
                ),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        tmp.add_entry_point(entry_point);
        tmp
    };

    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        None,
        Some(PACKAGE_HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    );

    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(HASH_KEY_NAME, contract_hash.into());
}
