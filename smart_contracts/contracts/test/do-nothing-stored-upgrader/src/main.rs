#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::{String, ToString};
use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use core::convert::TryInto;

use casper_types::{
    addressable_entity::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys},
    CLType, CLTyped, Key, PackageHash, Parameter, URef,
};

const ENTRY_FUNCTION_NAME: &str = "delegate";
const DO_NOTHING_PACKAGE_HASH_KEY_NAME: &str = "do_nothing_package_hash";
const DO_NOTHING_ACCESS_KEY_NAME: &str = "do_nothing_access";
const CONTRACT_VERSION: &str = "contract_version";
const ARG_PURSE_NAME: &str = "purse_name";

#[no_mangle]
pub extern "C" fn delegate() {
    let _named_keys = runtime::list_named_keys();
    runtime::put_key("called_do_nothing_ver_2", Key::Hash([1u8; 32]));
    create_purse_01::delegate()
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let delegate = EntryPoint::new(
            ENTRY_FUNCTION_NAME.to_string(),
            vec![Parameter::new(ARG_PURSE_NAME, String::cl_type())],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::AddressableEntity,
        );
        entry_points.add_entry_point(delegate);

        entry_points
    };

    let do_nothing_package_hash: PackageHash = runtime::get_key(DO_NOTHING_PACKAGE_HASH_KEY_NAME)
        .unwrap_or_revert()
        .into_package_addr()
        .unwrap_or_revert()
        .into();

    let _do_nothing_uref: URef = runtime::get_key(DO_NOTHING_ACCESS_KEY_NAME)
        .unwrap_or_revert()
        .try_into()
        .unwrap_or_revert();

    let (contract_hash, contract_version) =
        storage::add_contract_version(do_nothing_package_hash, entry_points, NamedKeys::new());
    runtime::put_key(CONTRACT_VERSION, storage::new_uref(contract_version).into());
    runtime::put_key("end of upgrade", Key::contract_entity_key(contract_hash));
}
