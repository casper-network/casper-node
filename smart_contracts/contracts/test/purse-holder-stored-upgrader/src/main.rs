#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::String;

use casperlabs_contract::{
    contract_api::{runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    contracts::NamedKeys, CLType, CLValue, ContractPackageHash, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Parameter, URef,
};

pub const METHOD_ADD: &str = "add";
pub const METHOD_REMOVE: &str = "remove";
pub const METHOD_VERSION: &str = "version";
pub const ARG_PURSE_NAME: &str = "purse_name";
pub const NEW_VERSION: &str = "1.0.1";
const VERSION: &str = "version";
const ACCESS_KEY_NAME: &str = "purse_holder_access";
const PURSE_HOLDER_STORED_CONTRACT_NAME: &str = "purse_holder_stored";
const ARG_CONTRACT_PACKAGE: &str = "contract_package";
const CONTRACT_VERSION: &str = "contract_version";

fn purse_name() -> String {
    runtime::get_named_arg(ARG_PURSE_NAME)
}

#[no_mangle]
pub extern "C" fn add() {
    let purse_name = purse_name();
    let purse = system::create_purse();
    runtime::put_key(&purse_name, purse.into());
}

#[no_mangle]
pub extern "C" fn remove() {
    let purse_name = purse_name();
    runtime::remove_key(&purse_name);
}

#[no_mangle]
pub extern "C" fn version() {
    runtime::ret(CLValue::from_t(VERSION).unwrap_or_revert())
}

#[no_mangle]
pub extern "C" fn call() {
    let contract_package: ContractPackageHash = runtime::get_named_arg(ARG_CONTRACT_PACKAGE);
    let _access_key: URef = runtime::get_key(ACCESS_KEY_NAME)
        .expect("should have access key")
        .into_uref()
        .expect("should be uref");

    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let add = EntryPoint::new(
            METHOD_ADD,
            vec![Parameter::new(ARG_PURSE_NAME, CLType::String)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(add);
        let version = EntryPoint::new(
            METHOD_VERSION,
            vec![],
            CLType::String,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(version);

        let remove = EntryPoint::new(
            METHOD_REMOVE,
            vec![Parameter::new(ARG_PURSE_NAME, CLType::String)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );
        entry_points.add_entry_point(remove);
        entry_points
    };
    // this should overwrite the previous contract obj with the new contract obj at the same uref
    let (new_contract_hash, new_contract_version) =
        storage::add_contract_version(contract_package, entry_points, NamedKeys::new());
    runtime::put_key(PURSE_HOLDER_STORED_CONTRACT_NAME, new_contract_hash.into());
    runtime::put_key(
        CONTRACT_VERSION,
        storage::new_uref(new_contract_version).into(),
    );
    // set new version
    let version_key = storage::new_uref(NEW_VERSION).into();
    runtime::put_key(VERSION, version_key);
}
