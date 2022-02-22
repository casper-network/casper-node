#![no_std]
#![no_main]

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
const PUT_KEY_AS_SESSION: &str = "put_key_as_session";
const PUT_KEY_AS_CONTRACT: &str = "put_key_as_contract";
const READ_AS_SESSION: &str = "read_as_session";
const READ_AS_CONTRACT: &str = "read_as_contract";
const WRITE_AS_SESSION: &str = "write_as_session";
const WRITE_AS_CONTRACT: &str = "write_as_contract";
const ADD_AS_SESSION: &str = "add_as_session";
const ADD_AS_CONTRACT: &str = "add_as_contract";
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
    entry_points.add_entry_point(EntryPoint::new(
        PUT_KEY_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        PUT_KEY_AS_CONTRACT,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        READ_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        READ_AS_CONTRACT,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        WRITE_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        WRITE_AS_CONTRACT,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        ADD_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    entry_points.add_entry_point(EntryPoint::new(
        ADD_AS_CONTRACT,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
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

#[no_mangle]
pub extern "C" fn write_as_contract() {
    let uref = URef::default().into_read_add_write();
    storage::write(uref, ());
}

#[no_mangle]
pub extern "C" fn write_as_session() {
    let uref = URef::default().with_access_rights(AccessRights::READ_ADD_WRITE);
    storage::write(uref, ());
}

#[no_mangle]
pub extern "C" fn read_as_contract() {
    let uref = URef::default().into_read_add_write();
    let _: Option<()> = storage::read(uref).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn read_as_session() {
    let uref = URef::default().with_access_rights(AccessRights::READ_ADD_WRITE);
    let _: Option<()> = storage::read(uref).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn put_key_as_contract() {
    let uref = URef::default().into_read_add_write();
    runtime::put_key("", uref.into());
}

#[no_mangle]
pub extern "C" fn put_key_as_session() {
    let uref = URef::default().with_access_rights(AccessRights::READ_ADD_WRITE);
    runtime::put_key("", uref.into());
}

#[no_mangle]
pub extern "C" fn add_as_contract() {
    let uref = URef::default().into_read_add_write();
    storage::write(uref, ());
}

#[no_mangle]
pub extern "C" fn add_as_session() {
    let uref = URef::default().with_access_rights(AccessRights::READ_ADD_WRITE);
    storage::write(uref, ());
}
