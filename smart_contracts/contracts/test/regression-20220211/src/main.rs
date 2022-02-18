#![no_std]
#![no_main]

extern crate alloc;

use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::Parameters, AccessRights, CLType, CLValue, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, URef,
};

const RET_AS_CONTRACT: &str = "ret_as_contract";
const RET_AS_SESSION: &str = "ret_as_session";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";

#[no_mangle]
pub extern "C" fn call() {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        RET_AS_CONTRACT,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    entry_points.add_entry_point(EntryPoint::new(
        RET_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    ));

    let (contract_hash, _contract_version) =
        storage::new_locked_contract(entry_points, None, None, None);

    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}

#[no_mangle]
pub extern "C" fn ret_as_contract() {
    let uref = URef::default().into_read_add_write();
    runtime::ret(CLValue::from_t(uref).unwrap_or_revert())
}

#[no_mangle]
pub extern "C" fn ret_as_session() {
    let uref = URef::default().with_access_rights(AccessRights::READ_ADD_WRITE);
    runtime::ret(CLValue::from_t(uref).unwrap_or_revert());
}
