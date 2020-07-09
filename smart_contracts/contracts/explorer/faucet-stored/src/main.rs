#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::ToString, vec};

use contract::contract_api::{runtime, storage};
use types::{
    account::AccountHash, CLType, CLTyped, ContractHash, ContractVersion, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

const CONTRACT_NAME: &str = "faucet";
const HASH_KEY_NAME: &str = "faucet_package";
const ACCESS_KEY_NAME: &str = "faucet_package_access";
const ENTRY_POINT_NAME: &str = "call_faucet";
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";
const CONTRACT_VERSION: &str = "contract_version";

#[no_mangle]
pub extern "C" fn call_faucet() {
    faucet::delegate();
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
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key(CONTRACT_NAME, contract_hash.into());
}
