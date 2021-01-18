#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{
    account::AccountHash, CLType, CLTyped, ContractHash, ContractVersion, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

const CONTRACT_NAME: &str = "transfer_to_account";
const CONTRACT_VERSION_KEY: &str = "contract_version";
const FUNCTION_NAME: &str = "transfer";

const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

#[no_mangle]
pub extern "C" fn transfer() {
    transfer_to_account::delegate();
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            FUNCTION_NAME,
            vec![
                Parameter::new(ARG_TARGET, AccountHash::cl_type()),
                Parameter::new(ARG_AMOUNT, CLType::U512),
            ],
            CLType::URef,
            EntryPointAccess::Public,
            EntryPointType::Session,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    storage::new_contract(entry_points, None, None, None)
}

#[no_mangle]
pub extern "C" fn call() {
    let (contract_hash, contract_version) = store();
    let version_uref = storage::new_uref(contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());
    runtime::put_key(CONTRACT_NAME, contract_hash.into());
}
