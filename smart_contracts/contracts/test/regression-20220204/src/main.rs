#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::{collections::BTreeMap, string::String};
use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    contracts::{NamedKeys, Parameters},
    CLType, CLTyped, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, Parameter,
    URef, U512,
};

const TRANSFER_AS_CONTRACT: &str = "transfer_as_contract";
const TRANSFER_AS_SESSION: &str = "transfer_as_session";
const TRANSFER_MAIN_PURSE_AS_SESSION: &str = "transfer_main_purse_as_session";
const NONTRIVIAL_ARG_AS_CONTRACT: &str = "nontrivial_arg_as_contract";
const ARG_PURSE: &str = "purse";
const PURSE_KEY: &str = "purse";
const CONTRACT_HASH_NAME: &str = "regression-contract-hash";
const PACKAGE_HASH_NAME: &str = "package-contract-hash";
type NonTrivialArg = BTreeMap<String, Key>;

#[no_mangle]
pub extern "C" fn call() {
    let (contract_package_hash, _access_uref) = storage::create_contract_package_at_hash();

    runtime::put_key(PACKAGE_HASH_NAME, contract_package_hash.into());

    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        TRANSFER_AS_CONTRACT,
        vec![Parameter::new(ARG_PURSE, URef::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    entry_points.add_entry_point(EntryPoint::new(
        TRANSFER_AS_SESSION,
        vec![Parameter::new(ARG_PURSE, URef::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    ));

    type NonTrivialArg = BTreeMap<String, Key>;

    entry_points.add_entry_point(EntryPoint::new(
        NONTRIVIAL_ARG_AS_CONTRACT,
        vec![Parameter::new(ARG_PURSE, NonTrivialArg::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));

    entry_points.add_entry_point(EntryPoint::new(
        TRANSFER_MAIN_PURSE_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    ));

    entry_points.add_entry_point(EntryPoint::new(
        TRANSFER_MAIN_PURSE_AS_SESSION,
        Parameters::new(),
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    ));

    let named_keys = {
        let mut named_keys = NamedKeys::new();
        let purse = system::create_purse();
        named_keys.insert(PURSE_KEY.into(), purse.into());
        named_keys
    };

    let (contract_hash, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}

#[no_mangle]
pub extern "C" fn transfer_as_contract() {
    let source_purse: URef = runtime::get_named_arg(ARG_PURSE);
    let target_purse = runtime::get_key(PURSE_KEY)
        .unwrap_or_revert()
        .into_uref()
        .unwrap_or_revert();

    assert!(
        !source_purse.is_writeable(),
        "Host should modify write bits in passed main purse"
    );
    assert!(runtime::is_valid_uref(source_purse));

    let extended = source_purse.into_read_add_write();
    assert!(!runtime::is_valid_uref(extended));

    system::transfer_from_purse_to_purse(extended, target_purse, U512::one(), Some(42))
        .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn transfer_as_session() {
    let source_purse: URef = runtime::get_named_arg(ARG_PURSE);

    assert!(!source_purse.is_writeable());

    assert!(runtime::is_valid_uref(source_purse));
    let extended = source_purse.into_read_add_write();
    assert!(runtime::is_valid_uref(extended));

    system::transfer_from_purse_to_account(
        extended,
        AccountHash::new([0; 32]),
        U512::one(),
        Some(42),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn transfer_main_purse_as_session() {
    let source_purse: URef = account::get_main_purse();

    assert!(runtime::is_valid_uref(source_purse));
    let extended = source_purse.into_write();
    assert!(runtime::is_valid_uref(extended));

    system::transfer_from_purse_to_account(
        extended,
        AccountHash::new([0; 32]),
        U512::one(),
        Some(42),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn nontrivial_arg_as_contract() {
    let non_trivial_arg: NonTrivialArg = runtime::get_named_arg(ARG_PURSE);
    let source_purse: URef = non_trivial_arg
        .into_values()
        .filter_map(Key::into_uref)
        .next()
        .unwrap();

    let target_purse = runtime::get_key(PURSE_KEY)
        .unwrap_or_revert()
        .into_uref()
        .unwrap_or_revert();

    assert!(!source_purse.is_writeable());
    assert!(runtime::is_valid_uref(source_purse));

    let extended = source_purse.into_read_add_write();
    assert!(!runtime::is_valid_uref(extended));

    system::transfer_from_purse_to_purse(extended, target_purse, U512::one(), Some(42))
        .unwrap_or_revert();
}
