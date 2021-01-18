#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::ToString;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    account::AccountHash, contracts::ContractHash, CLType, CLTyped, ContractVersion, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

const CONTRACT_NAME: &str = "transfer_to_account";
const CONTRACT_VERSION_KEY: &str = "contract_version";
const ENTRY_POINT_NAME: &str = "transfer";
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

const HASH_KEY_NAME: &str = "transfer_to_account_U512";
const ACCESS_KEY_NAME: &str = "transfer_to_account_U512_access";

#[no_mangle]
pub extern "C" fn transfer() {
    transfer_to_account_u512::delegate();
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            ENTRY_POINT_NAME,
            vec![
                Parameter::new(ARG_TARGET, AccountHash::cl_type()),
                Parameter::new(ARG_AMOUNT, CLType::U512),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    storage::new_contract(
        entry_points,
        None,
        Some(HASH_KEY_NAME.to_string()),
        Some(ACCESS_KEY_NAME.to_string()),
    )
}

#[no_mangle]
pub extern "C" fn call() {
    let (contract_hash, contract_version) = store();
    let version_uref = storage::new_uref(contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());
    runtime::put_key(CONTRACT_NAME, contract_hash.into());
}
