#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

mod private_chain_support;

use alloc::{string::String, vec::Vec};
use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    contracts::{NamedKeys, Parameters},
    CLType, CLTyped, CLValue, ContractHash, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Key, Parameter,
};

const HASH_KEY_NAME: &str = "package_hash";
const ACCESS_KEY_NAME: &str = "access_key";
const CONTRACT_VERSION_KEY: &str = "contract_version";
const CONTRACT_HASH_NAME: &str = "contract_hash";

const DISABLE_ACCOUNT_ENTRYPOINT: &str = "disable_account";
const ENABLE_ACCOUNT_ENTRYPOINT: &str = "enable_account";
const ARG_ACCOUNT_HASH: &str = "account_hash";

const DISABLE_CONTRACT_ENTRYPOINT: &str = "disable_contract";
const ENABLE_CONTRACT_ENTRYPOINT: &str = "enable_contract";
const ARG_CONTRACT_HASH: &str = "contract_hash";

#[no_mangle]
pub extern "C" fn disable_account() {
    let account_hash: AccountHash = runtime::get_named_arg(ARG_ACCOUNT_HASH);
    private_chain_support::control_management(Key::Account(account_hash), false).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn enable_account() {
    let account_hash: AccountHash = runtime::get_named_arg(ARG_ACCOUNT_HASH);
    private_chain_support::control_management(Key::Account(account_hash), true).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn disable_contract() {
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
    private_chain_support::control_management(Key::from(contract_hash), false).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn enable_contract() {
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
    private_chain_support::control_management(Key::from(contract_hash), true).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn test() {
    let words = vec!["hello", "world"];
    runtime::ret(CLValue::from_t(words).unwrap_or_revert());
}

#[no_mangle]
pub extern "C" fn call() {
    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let entry_points = get_entry_points();

    let named_keys = NamedKeys::new();

    let (contract_hash, contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    let version_uref = storage::new_uref(contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());
    runtime::put_key(CONTRACT_HASH_NAME, contract_hash.into());
}

fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let disable_account_entrypoint = EntryPoint::new(
        DISABLE_ACCOUNT_ENTRYPOINT,
        vec![Parameter::new(ARG_ACCOUNT_HASH, AccountHash::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(disable_account_entrypoint);

    let enable_account_entrypoint = EntryPoint::new(
        ENABLE_ACCOUNT_ENTRYPOINT,
        vec![Parameter::new(ARG_ACCOUNT_HASH, AccountHash::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(enable_account_entrypoint);

    let enable_contract_entrypoint = EntryPoint::new(
        ENABLE_CONTRACT_ENTRYPOINT,
        vec![Parameter::new(ARG_CONTRACT_HASH, ContractHash::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(enable_contract_entrypoint);

    let disable_contract_entrypoint = EntryPoint::new(
        DISABLE_CONTRACT_ENTRYPOINT,
        vec![Parameter::new(ARG_CONTRACT_HASH, ContractHash::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(disable_contract_entrypoint);

    let test = EntryPoint::new(
        "test",
        Parameters::new(),
        <Vec<String>>::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(test);

    entry_points
}
