#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::{
    boxed::Box,
    collections::BTreeSet,
    string::{String, ToString},
    vec::Vec,
};

use core::{convert::TryInto, iter::FromIterator, mem::MaybeUninit};

use casper_contract::{
    contract_api::{self, runtime, storage},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    api_error,
    bytesrepr::{self, ToBytes},
    contracts::{EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, NamedKeys},
    ApiError, CLType, ContractPackage, ContractPackageHash, Group, Key, Parameter, URef,
};

const PACKAGE_HASH_KEY: &str = "package_hash_key";
const PACKAGE_ACCESS_KEY: &str = "package_access_key";
const CREATE_GROUP: &str = "create_group";
const REMOVE_GROUP: &str = "remove_group";
const EXTEND_GROUP_UREFS: &str = "extend_group_urefs";
const REMOVE_GROUP_UREFS: &str = "remove_group_urefs";
const GROUP_NAME_ARG: &str = "group_name";
const UREFS_ARG: &str = "urefs";
const TOTAL_NEW_UREFS_ARG: &str = "total_new_urefs";
const TOTAL_EXISTING_UREFS_ARG: &str = "total_existing_urefs";
const UREF_INDICES_ARG: &str = "uref_indices";

#[no_mangle]
pub extern "C" fn create_group() {
    let package_hash_key: ContractPackageHash = runtime::get_key(PACKAGE_HASH_KEY)
        .and_then(Key::into_hash)
        .unwrap_or_revert()
        .into();
    let group_name: String = runtime::get_named_arg(GROUP_NAME_ARG);
    let total_urefs: u64 = runtime::get_named_arg(TOTAL_NEW_UREFS_ARG);
    let total_existing_urefs: u64 = runtime::get_named_arg(TOTAL_EXISTING_UREFS_ARG);
    let existing_urefs: Vec<URef> = (0..total_existing_urefs).map(storage::new_uref).collect();

    storage::create_contract_user_group(
        package_hash_key,
        &group_name,
        total_urefs as u8,
        BTreeSet::from_iter(existing_urefs),
    )
    .unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn remove_group() {
    let package_hash_key: ContractPackageHash = runtime::get_key(PACKAGE_HASH_KEY)
        .and_then(Key::into_hash)
        .unwrap_or_revert()
        .into();
    let group_name: String = runtime::get_named_arg(GROUP_NAME_ARG);
    storage::remove_contract_user_group(package_hash_key, &group_name).unwrap_or_revert();
}

#[no_mangle]
pub extern "C" fn extend_group_urefs() {
    let package_hash_key: ContractPackageHash = runtime::get_key(PACKAGE_HASH_KEY)
        .and_then(Key::into_hash)
        .unwrap_or_revert()
        .into();
    let group_name: String = runtime::get_named_arg(GROUP_NAME_ARG);
    let new_urefs_count: u64 = runtime::get_named_arg(TOTAL_NEW_UREFS_ARG);

    // Provisions additional urefs inside group
    for _ in 1..=new_urefs_count {
        let _new_uref = storage::provision_contract_user_group_uref(package_hash_key, &group_name)
            .unwrap_or_revert();
    }
}

fn read_host_buffer_into(dest: &mut [u8]) -> Result<usize, ApiError> {
    let mut bytes_written = MaybeUninit::uninit();
    let ret = unsafe {
        ext_ffi::casper_read_host_buffer(dest.as_mut_ptr(), dest.len(), bytes_written.as_mut_ptr())
    };
    // NOTE: When rewriting below expression as `result_from(ret).map(|_| unsafe { ... })`, and the
    // caller ignores the return value, execution of the contract becomes unstable and ultimately
    // leads to `Unreachable` error.
    api_error::result_from(ret)?;
    Ok(unsafe { bytes_written.assume_init() })
}

fn read_contract_package(
    package_hash: ContractPackageHash,
) -> Result<Option<ContractPackage>, ApiError> {
    let key = Key::from(package_hash);
    let (key_ptr, key_size, _bytes) = {
        let bytes = key.into_bytes().unwrap_or_revert();
        let ptr = bytes.as_ptr();
        let size = bytes.len();
        (ptr, size, bytes)
    };

    let value_size = {
        let mut value_size = MaybeUninit::uninit();
        let ret = unsafe { ext_ffi::casper_read_value(key_ptr, key_size, value_size.as_mut_ptr()) };
        match api_error::result_from(ret) {
            Ok(_) => unsafe { value_size.assume_init() },
            Err(ApiError::ValueNotFound) => return Ok(None),
            Err(e) => runtime::revert(e),
        }
    };

    let value_bytes = {
        let mut dest: Vec<u8> = if value_size == 0 {
            Vec::new()
        } else {
            let bytes_non_null_ptr = contract_api::alloc_bytes(value_size);
            unsafe { Vec::from_raw_parts(bytes_non_null_ptr.as_ptr(), value_size, value_size) }
        };
        read_host_buffer_into(&mut dest)?;
        dest
    };

    Ok(Some(bytesrepr::deserialize(value_bytes)?))
}

#[no_mangle]
pub extern "C" fn remove_group_urefs() {
    let package_hash: ContractPackageHash = runtime::get_key(PACKAGE_HASH_KEY)
        .and_then(Key::into_hash)
        .unwrap_or_revert()
        .into();
    let _package_access_key: URef = runtime::get_key(PACKAGE_ACCESS_KEY)
        .unwrap_or_revert()
        .try_into()
        .unwrap();
    let group_name: String = runtime::get_named_arg(GROUP_NAME_ARG);
    let ordinals: Vec<u64> = runtime::get_named_arg(UREF_INDICES_ARG);

    let contract_package: ContractPackage = read_contract_package(package_hash)
        .unwrap_or_revert()
        .unwrap_or_revert();

    let group_urefs = contract_package
        .groups()
        .get(&Group::new("Group 1"))
        .unwrap_or_revert();
    let group_urefs_vec = Vec::from_iter(group_urefs);

    let mut urefs_to_remove = BTreeSet::new();
    for ordinal in ordinals {
        urefs_to_remove.insert(
            group_urefs_vec
                .get(ordinal as usize)
                .cloned()
                .cloned()
                .unwrap_or_revert(),
        );
    }

    storage::remove_contract_user_group_urefs(package_hash, &group_name, urefs_to_remove)
        .unwrap_or_revert();
}

/// Restricted uref comes from creating a group and will be assigned to a smart contract
fn create_entry_points_1() -> EntryPoints {
    let mut entry_points = EntryPoints::new();
    let restricted_session = EntryPoint::new(
        CREATE_GROUP.to_string(),
        vec![
            Parameter::new(GROUP_NAME_ARG, CLType::String),
            Parameter::new(TOTAL_EXISTING_UREFS_ARG, CLType::U64),
            Parameter::new(TOTAL_NEW_UREFS_ARG, CLType::U64),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(restricted_session);

    let remove_group = EntryPoint::new(
        REMOVE_GROUP.to_string(),
        vec![Parameter::new(GROUP_NAME_ARG, CLType::String)],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(remove_group);

    let entry_point_name = EXTEND_GROUP_UREFS.to_string();
    let extend_group_urefs = EntryPoint::new(
        entry_point_name,
        vec![
            Parameter::new(GROUP_NAME_ARG, CLType::String),
            Parameter::new(TOTAL_NEW_UREFS_ARG, CLType::U64),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(extend_group_urefs);

    let entry_point_name = REMOVE_GROUP_UREFS.to_string();
    let remove_group_urefs = EntryPoint::new(
        entry_point_name,
        vec![
            Parameter::new(GROUP_NAME_ARG, CLType::String),
            Parameter::new(UREFS_ARG, CLType::List(Box::new(CLType::URef))),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(remove_group_urefs);
    entry_points
}

fn install_version_1(package_hash: ContractPackageHash) {
    let contract_named_keys = NamedKeys::new();

    let entry_points = create_entry_points_1();
    storage::add_contract_version(package_hash, entry_points, contract_named_keys);
}

#[no_mangle]
pub extern "C" fn call() {
    let (package_hash, access_uref) = storage::create_contract_package_at_hash();

    runtime::put_key(PACKAGE_HASH_KEY, package_hash.into());
    runtime::put_key(PACKAGE_ACCESS_KEY, access_uref.into());

    install_version_1(package_hash);
}
