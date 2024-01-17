#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::mem::MaybeUninit;

use casper_contract::{
    contract_api,
    contract_api::{account, runtime, storage, system},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::{AccountHash, ActionType, Weight},
    api_error,
    bytesrepr::ToBytes,
    contracts::MAX_GROUPS,
    runtime_args, AccessRights, ApiError, CLType, CLValue, ContractHash, ContractPackageHash,
    EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, EraId, Key, Parameter, RuntimeArgs,
    TransferredTo, URef, U512,
};

const NOOP: &str = "noop";

fn to_ptr<T: ToBytes>(t: &T) -> (*const u8, usize, Vec<u8>) {
    let bytes = t.to_bytes().unwrap_or_revert();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size, bytes)
}

#[no_mangle]
extern "C" fn noop() {}

fn store_noop_contract(maybe_contract_pkg_hash: Option<ContractPackageHash>) -> ContractHash {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        NOOP,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    match maybe_contract_pkg_hash {
        Some(contract_pkg_hash) => {
            let (contract_hash, _version) =
                storage::add_contract_version(contract_pkg_hash, entry_points, BTreeMap::new());
            contract_hash
        }
        None => {
            let (contract_hash, _version) = storage::new_contract(entry_points, None, None, None);
            contract_hash
        }
    }
}

fn get_name() -> String {
    let large_name: bool = runtime::get_named_arg("large_name");
    if large_name {
        "a".repeat(10_000)
    } else {
        "a".to_string()
    }
}

fn get_named_arg_size(name: &str) -> usize {
    let mut arg_size: usize = 0;
    let ret = unsafe {
        ext_ffi::casper_get_named_arg_size(
            name.as_bytes().as_ptr(),
            name.len(),
            &mut arg_size as *mut usize,
        )
    };
    api_error::result_from(ret).unwrap_or_revert();
    arg_size
}

#[no_mangle]
pub extern "C" fn call() {
    let fn_arg: String = runtime::get_named_arg("fn");
    match fn_arg.as_str() {
        "write" => {
            let len: u32 = runtime::get_named_arg("len");
            let uref = storage::new_uref(());
            let key = Key::from(uref);
            let (key_ptr, key_size, _bytes1) = to_ptr(&key);
            let value = vec![u8::MAX; len as usize];
            let cl_value = CLValue::from_t(value).unwrap_or_revert();
            let (cl_value_ptr, cl_value_size, _bytes2) = to_ptr(&cl_value);
            for _i in 0..u64::MAX {
                unsafe {
                    ext_ffi::casper_write(key_ptr, key_size, cl_value_ptr, cl_value_size);
                }
            }
        }
        "read" => {
            let len: Option<u32> = runtime::get_named_arg("len");
            let key = match len {
                Some(len) => {
                    let key = Key::URef(storage::new_uref(()));
                    let uref = storage::new_uref(());
                    storage::write(uref, vec![u8::MAX; len as usize]);
                    key
                }
                None => Key::Hash([0; 32]),
            };
            let key_bytes = key.into_bytes().unwrap();
            let key_ptr = key_bytes.as_ptr();
            let key_size = key_bytes.len();
            let mut buffer = vec![0; len.unwrap_or_default() as usize];
            for _i in 0..u64::MAX {
                let mut value_size = MaybeUninit::uninit();
                let ret = unsafe {
                    ext_ffi::casper_read_value(key_ptr, key_size, value_size.as_mut_ptr())
                };
                // If we actually read a value, we need to clear the host buffer before trying to
                // read another value.
                if len.is_some() {
                    assert_eq!(ret, 0);
                } else {
                    assert_eq!(ret, u32::from(ApiError::ValueNotFound) as i32);
                    continue;
                }
                unsafe {
                    value_size.assume_init();
                }
                let mut bytes_written = MaybeUninit::uninit();
                let ret = unsafe {
                    ext_ffi::casper_read_host_buffer(
                        buffer.as_mut_ptr(),
                        buffer.len(),
                        bytes_written.as_mut_ptr(),
                    )
                };
                assert_eq!(ret, 0);
            }
        }
        "add" => {
            let large: bool = runtime::get_named_arg("large");
            if large {
                let uref = storage::new_uref(U512::zero());
                for _i in 0..u64::MAX {
                    storage::add(uref, U512::MAX)
                }
            } else {
                let uref = storage::new_uref(0_i32);
                for _i in 0..u64::MAX {
                    storage::add(uref, 1_i32)
                }
            }
        }
        "new" => {
            let len: u32 = runtime::get_named_arg("len");
            for _i in 0..u64::MAX {
                let _n = storage::new_uref(vec![u32::MAX; len as usize]);
            }
        }
        "call_contract" => {
            let args_len: u32 = runtime::get_named_arg("args_len");
            let args = runtime_args! { "a" => vec![u8::MAX; args_len as usize] };
            let contract_hash = store_noop_contract(None);
            let (contract_hash_ptr, contract_hash_size, _bytes1) = to_ptr(&contract_hash);
            let (entry_point_name_ptr, entry_point_name_size, _bytes2) = to_ptr(&NOOP);
            let (runtime_args_ptr, runtime_args_size, _bytes3) = to_ptr(&args);
            let mut bytes_written = MaybeUninit::uninit();
            for _i in 0..u64::MAX {
                let ret = unsafe {
                    ext_ffi::casper_call_contract(
                        contract_hash_ptr,
                        contract_hash_size,
                        entry_point_name_ptr,
                        entry_point_name_size,
                        runtime_args_ptr,
                        runtime_args_size,
                        bytes_written.as_mut_ptr(),
                    )
                };
                api_error::result_from(ret).unwrap_or_revert();
            }
        }
        "get_key" => {
            let maybe_large_key: Option<bool> = runtime::get_named_arg("large_key");
            match maybe_large_key {
                Some(large_key) => {
                    let name = get_name();
                    let key = if large_key {
                        let uref = storage::new_uref(());
                        Key::URef(uref)
                    } else {
                        Key::EraInfo(EraId::new(0))
                    };
                    runtime::put_key(&name, key);
                    for _i in 0..u64::MAX {
                        let _k = runtime::get_key(&name);
                    }
                }
                None => {
                    for i in 0..u64::MAX {
                        let _k = runtime::get_key(i.to_string().as_str());
                    }
                }
            }
        }
        "has_key" => {
            let exists: bool = runtime::get_named_arg("key_exists");
            if exists {
                let name = get_name();
                runtime::put_key(&name, Key::EraInfo(EraId::new(0)));
                for _i in 0..u64::MAX {
                    let _b = runtime::has_key(&name);
                }
            } else {
                for i in 0..u64::MAX {
                    let _b = runtime::has_key(i.to_string().as_str());
                }
            }
        }
        "put_key" => {
            let base_name = get_name();
            let large_key: bool = runtime::get_named_arg("large_key");
            let key = if large_key {
                let uref = storage::new_uref(());
                Key::URef(uref)
            } else {
                Key::EraInfo(EraId::new(0))
            };
            let maybe_num_keys: Option<u32> = runtime::get_named_arg("num_keys");
            let num_keys = maybe_num_keys.unwrap_or(u32::MAX);
            for i in 0..num_keys {
                runtime::put_key(format!("{base_name}{i}").as_str(), key);
            }
        }
        "is_valid_uref" => {
            let valid: bool = runtime::get_named_arg("valid");
            let uref = if valid {
                storage::new_uref(())
            } else {
                URef::new([1; 32], AccessRights::default())
            };
            for _i in 0..u64::MAX {
                let is_valid = runtime::is_valid_uref(uref);
                assert_eq!(valid, is_valid);
            }
        }
        "add_associated_key" => {
            let remove_after_adding: bool = runtime::get_named_arg("remove_after_adding");
            let account_hash = AccountHash::new([1; 32]);
            let weight = Weight::new(1);
            for _i in 0..u64::MAX {
                if remove_after_adding {
                    account::add_associated_key(account_hash, weight).unwrap_or_revert();
                    // Remove to avoid getting a duplicate key error on next iteration.
                    account::remove_associated_key(account_hash).unwrap_or_revert();
                } else {
                    let _e = account::add_associated_key(account_hash, weight);
                }
            }
        }
        "remove_associated_key" => {
            for _i in 0..u64::MAX {
                account::remove_associated_key(AccountHash::new([1; 32])).unwrap_err();
            }
        }
        "update_associated_key" => {
            let exists: bool = runtime::get_named_arg("exists");
            let account_hash = AccountHash::new([1; 32]);
            if exists {
                account::add_associated_key(account_hash, Weight::new(1)).unwrap_or_revert();
                for i in 0..u64::MAX {
                    account::update_associated_key(account_hash, Weight::new(i as u8))
                        .unwrap_or_revert();
                }
            } else {
                for i in 0..u64::MAX {
                    account::update_associated_key(account_hash, Weight::new(i as u8)).unwrap_err();
                }
            }
        }
        "set_action_threshold" => {
            for _i in 0..u64::MAX {
                account::set_action_threshold(ActionType::Deployment, Weight::new(1))
                    .unwrap_or_revert();
            }
        }
        "load_named_keys" => {
            let num_keys: u32 = runtime::get_named_arg("num_keys");
            if num_keys == 0 {
                for _i in 0..u64::MAX {
                    assert!(runtime::list_named_keys().is_empty());
                }
                return;
            }
            // Where `num_keys` > 0, we should have put the required number of named keys in a
            // previous execution via the `put_key` flow of this contract.
            for _i in 0..u64::MAX {
                assert_eq!(runtime::list_named_keys().len() as u32, num_keys);
            }
        }
        "remove_key" => {
            let name = get_name();
            for _i in 0..u64::MAX {
                runtime::remove_key(&name)
            }
        }
        "get_caller" => {
            for _i in 0..u64::MAX {
                let _c = runtime::get_caller();
            }
        }
        "get_blocktime" => {
            for _i in 0..u64::MAX {
                let _b = runtime::get_blocktime();
            }
        }
        "create_purse" => {
            for _i in 0..u64::MAX {
                let _u = system::create_purse();
            }
        }
        "transfer_to_account" => {
            let account_exists: bool = runtime::get_named_arg("account_exists");
            let amount = U512::one();
            let id = Some(u64::MAX);
            if account_exists {
                let target = AccountHash::new([1; 32]);
                let to = system::transfer_to_account(target, amount, id).unwrap_or_revert();
                assert_eq!(to, TransferredTo::NewAccount);
                for _i in 0..u64::MAX {
                    let to = system::transfer_to_account(target, amount, id).unwrap_or_revert();
                    assert_eq!(to, TransferredTo::ExistingAccount);
                }
            } else {
                let mut array = [0_u8; 32];
                for index in 0..32 {
                    for i in 1..=u8::MAX {
                        array[index] = i;
                        let target = AccountHash::new(array);
                        let to = system::transfer_to_account(target, amount, id).unwrap_or_revert();
                        assert_eq!(to, TransferredTo::NewAccount);
                    }
                }
            }
        }
        "transfer_from_purse_to_account" => {
            let account_exists: bool = runtime::get_named_arg("account_exists");
            let source = account::get_main_purse();
            let amount = U512::one();
            let id = Some(u64::MAX);
            if account_exists {
                let target = AccountHash::new([1; 32]);
                let to = system::transfer_to_account(target, amount, id).unwrap_or_revert();
                assert_eq!(to, TransferredTo::NewAccount);
                for _i in 0..u64::MAX {
                    let to = system::transfer_from_purse_to_account(source, target, amount, id)
                        .unwrap_or_revert();
                    assert_eq!(to, TransferredTo::ExistingAccount);
                }
            } else {
                let mut array = [0_u8; 32];
                for index in 0..32 {
                    for i in 1..=u8::MAX {
                        array[index] = i;
                        let target = AccountHash::new(array);
                        let to = system::transfer_from_purse_to_account(source, target, amount, id)
                            .unwrap_or_revert();
                        assert_eq!(to, TransferredTo::NewAccount);
                    }
                }
            }
        }
        "transfer_from_purse_to_purse" => {
            let source = account::get_main_purse();
            let target = system::create_purse();
            let amount = U512::one();
            let id = Some(u64::MAX);
            system::transfer_from_purse_to_purse(source, target, amount, id).unwrap_or_revert();
            for _i in 0..u64::MAX {
                system::transfer_from_purse_to_purse(source, target, amount, id).unwrap_or_revert();
            }
        }
        "get_balance" => {
            let purse_exists: bool = runtime::get_named_arg("purse_exists");
            let uref = if purse_exists {
                account::get_main_purse()
            } else {
                URef::new([1; 32], AccessRights::empty())
            };
            for _i in 0..u64::MAX {
                let maybe_balance = system::get_purse_balance(uref);
                assert_eq!(maybe_balance.is_some(), purse_exists);
            }
        }
        "get_phase" => {
            for _i in 0..u64::MAX {
                let _p = runtime::get_phase();
            }
        }
        "get_system_contract" => {
            for _i in 0..u64::MAX {
                let _h = system::get_mint();
            }
        }
        "get_main_purse" => {
            for _i in 0..u64::MAX {
                let _u = account::get_main_purse();
            }
        }
        "read_host_buffer" => {
            // The case where the host buffer is repeatedly filled is covered in the `read`
            // branch above.  All we do here is check repeatedly where `read_host_buffer` returns
            // `HostBufferEmpty`.
            let mut buffer = vec![0; 1];
            let mut bytes_written = MaybeUninit::uninit();
            for _i in 0..u64::MAX {
                let ret = unsafe {
                    ext_ffi::casper_read_host_buffer(
                        buffer.as_mut_ptr(),
                        buffer.len(),
                        bytes_written.as_mut_ptr(),
                    )
                };
                assert_eq!(ret, u32::from(ApiError::HostBufferEmpty) as i32);
            }
        }
        "create_contract_package_at_hash" => {
            for _i in 0..u64::MAX {
                let _h = storage::create_contract_package_at_hash();
            }
        }
        "add_contract_version" => {
            let entry_points_len: u32 = runtime::get_named_arg("entry_points_len");
            let mut entry_points = EntryPoints::new();
            for entry_point_index in 0..entry_points_len {
                entry_points.add_entry_point(EntryPoint::new(
                    format!("function_{entry_point_index}"),
                    vec![Parameter::new("a", CLType::PublicKey); 10],
                    CLType::Unit,
                    EntryPointAccess::Public,
                    EntryPointType::Contract,
                ));
            }
            let named_keys_len: u32 = runtime::get_named_arg("named_keys_len");
            let mut named_keys = BTreeMap::new();
            for named_key_index in 0..named_keys_len {
                let _ = named_keys.insert(named_key_index.to_string(), Key::Hash([1; 32]));
            }
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            for i in 1..u64::MAX {
                let (_h, version) = storage::add_contract_version(
                    contract_pkg_hash,
                    entry_points.clone(),
                    named_keys.clone(),
                );
                assert_eq!(version, i as u32);
            }
        }
        "disable_contract_version" => {
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            let (contract_hash, _version) = storage::add_contract_version(
                contract_pkg_hash,
                EntryPoints::new(),
                BTreeMap::new(),
            );
            for _i in 0..u64::MAX {
                storage::disable_contract_version(contract_pkg_hash, contract_hash)
                    .unwrap_or_revert();
            }
        }
        "call_versioned_contract" => {
            let args_len: u32 = runtime::get_named_arg("args_len");
            let args = runtime_args! { "a" => vec![u8::MAX; args_len as usize] };
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            let _ = store_noop_contract(Some(contract_pkg_hash));
            let (contract_pkg_hash_ptr, contract_pkg_hash_size, _bytes1) =
                to_ptr(&contract_pkg_hash);
            let (contract_version_ptr, contract_version_size, _bytes2) = to_ptr(&Some(1_u32));
            let (entry_point_name_ptr, entry_point_name_size, _bytes3) = to_ptr(&NOOP);
            let (runtime_args_ptr, runtime_args_size, _bytes4) = to_ptr(&args);
            let mut bytes_written = MaybeUninit::uninit();
            for _i in 0..u64::MAX {
                let ret = unsafe {
                    ext_ffi::casper_call_versioned_contract(
                        contract_pkg_hash_ptr,
                        contract_pkg_hash_size,
                        contract_version_ptr,
                        contract_version_size,
                        entry_point_name_ptr,
                        entry_point_name_size,
                        runtime_args_ptr,
                        runtime_args_size,
                        bytes_written.as_mut_ptr(),
                    )
                };
                api_error::result_from(ret).unwrap_or_revert();
            }
        }
        "create_contract_user_group" => {
            let label_len: u32 = runtime::get_named_arg("label_len");
            assert!(label_len > 0);
            let label_prefix: String = "a".repeat(label_len as usize - 1);
            let num_new_urefs: u8 = runtime::get_named_arg("num_new_urefs");
            let num_existing_urefs: u8 = runtime::get_named_arg("num_existing_urefs");
            let mut existing_urefs = BTreeSet::new();
            for _ in 0..num_existing_urefs {
                existing_urefs.insert(storage::new_uref(()));
            }
            let (existing_urefs_ptr, existing_urefs_size, _bytes1) = to_ptr(&existing_urefs);
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            let (contract_pkg_hash_ptr, contract_pkg_hash_size, _bytes2) =
                to_ptr(&contract_pkg_hash);
            let mut index = 0_u8;
            let mut label = String::new();
            let allow_exceeding_max_groups: bool =
                runtime::get_named_arg("allow_exceeding_max_groups");
            let expect_failure = num_new_urefs == u8::MAX || allow_exceeding_max_groups;
            let mut buffer = vec![0_u8; 5_000];
            let mut output_size = MaybeUninit::uninit();
            let mut bytes_written = MaybeUninit::uninit();
            loop {
                if index == MAX_GROUPS && !allow_exceeding_max_groups {
                    // We need to remove the group to avoid hitting the `contracts::MAX_GROUPS`
                    // limit (currently 10).
                    let result = storage::remove_contract_user_group(contract_pkg_hash, &label);
                    if !expect_failure {
                        result.unwrap_or_revert();
                    }
                } else {
                    label = format!("{label_prefix}{index}");
                    index += 1;
                }
                let (label_ptr, label_size, _bytes3) = to_ptr(&label);
                let ret = unsafe {
                    ext_ffi::casper_create_contract_user_group(
                        contract_pkg_hash_ptr,
                        contract_pkg_hash_size,
                        label_ptr,
                        label_size,
                        num_new_urefs,
                        existing_urefs_ptr,
                        existing_urefs_size,
                        output_size.as_mut_ptr(),
                    )
                };
                if !expect_failure {
                    api_error::result_from(ret).unwrap_or_revert();
                    let ret = unsafe {
                        ext_ffi::casper_read_host_buffer(
                            buffer.as_mut_ptr(),
                            buffer.len(),
                            bytes_written.as_mut_ptr(),
                        )
                    };
                    api_error::result_from(ret).unwrap_or_revert();
                }
            }
        }
        "print" => {
            let num_chars: u32 = runtime::get_named_arg("num_chars");
            let value: String = "a".repeat(num_chars as usize);
            for _i in 0..u64::MAX {
                runtime::print(&value);
            }
        }
        "get_runtime_arg_size" => {
            let name = "arg";
            for _i in 0..u64::MAX {
                let _s = get_named_arg_size(name);
            }
        }
        "get_runtime_arg" => {
            let name = "arg";
            let arg_size = get_named_arg_size(name);
            let data_non_null_ptr = contract_api::alloc_bytes(arg_size);
            for _i in 0..u64::MAX {
                let ret = unsafe {
                    ext_ffi::casper_get_named_arg(
                        name.as_bytes().as_ptr(),
                        name.len(),
                        data_non_null_ptr.as_ptr(),
                        arg_size,
                    )
                };
                api_error::result_from(ret).unwrap_or_revert();
            }
        }
        "remove_contract_user_group" => {
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            for _i in 0..u64::MAX {
                storage::remove_contract_user_group(contract_pkg_hash, "a").unwrap_err();
            }
        }
        "extend_contract_user_group_urefs" => {
            let allow_exceeding_max_urefs: bool =
                runtime::get_named_arg("allow_exceeding_max_urefs");
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            let label = "a";
            let _ =
                storage::create_contract_user_group(contract_pkg_hash, label, 0, BTreeSet::new())
                    .unwrap_or_revert();
            for _i in 0..u64::MAX {
                if allow_exceeding_max_urefs {
                    let _r = storage::provision_contract_user_group_uref(contract_pkg_hash, label);
                } else {
                    let uref =
                        storage::provision_contract_user_group_uref(contract_pkg_hash, label)
                            .unwrap_or_revert();
                    storage::remove_contract_user_group_urefs(
                        contract_pkg_hash,
                        label,
                        BTreeSet::from_iter(Some(uref)),
                    )
                    .unwrap_or_revert();
                }
            }
        }
        "remove_contract_user_group_urefs" => {
            // The success case is covered in `create_contract_user_group` above.  We only test
            // for unknown user groups here.
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            for _i in 0..u64::MAX {
                storage::remove_contract_user_group(contract_pkg_hash, "a").unwrap_err();
            }
        }
        "blake2b" => {
            let len: u32 = runtime::get_named_arg("len");
            let data = vec![1; len as usize];
            for _i in 0..u64::MAX {
                let _hash = runtime::blake2b(&data);
            }
        }
        "new_dictionary" => {
            let mut buffer = vec![0_u8; 33]; // bytesrepr-serialized length of URef
            for _i in 0..u64::MAX {
                let mut value_size = MaybeUninit::uninit();
                let ret = unsafe { ext_ffi::casper_new_dictionary(value_size.as_mut_ptr()) };
                api_error::result_from(ret).unwrap_or_revert();
                assert_eq!(buffer.len(), unsafe { value_size.assume_init() });
                let mut bytes_written = MaybeUninit::uninit();
                let ret = unsafe {
                    ext_ffi::casper_read_host_buffer(
                        buffer.as_mut_ptr(),
                        buffer.len(),
                        bytes_written.as_mut_ptr(),
                    )
                };
                assert_eq!(ret, 0);
            }
        }
        "dictionary_get" => {
            let name_len: u32 = runtime::get_named_arg("name_len");
            let name: String = "a".repeat(name_len as usize);
            let value_len: u32 = runtime::get_named_arg("value_len");
            let value = vec![u8::MAX; value_len as usize];
            let uref = storage::new_dictionary("a").unwrap_or_revert();
            storage::dictionary_put(uref, &name, value);

            for _i in 0..u64::MAX {
                let read_value: Vec<u8> = storage::dictionary_get(uref, &name)
                    .unwrap_or_revert()
                    .unwrap_or_revert();
                assert_eq!(read_value.len(), value_len as usize);
            }
        }
        "dictionary_put" => {
            let name_len: u32 = runtime::get_named_arg("name_len");
            let name: String = "a".repeat(name_len as usize);

            let value_len: u32 = runtime::get_named_arg("value_len");
            let value = vec![u8::MAX; value_len as usize];

            let uref = storage::new_dictionary("a").unwrap_or_revert();
            let (uref_ptr, uref_size, _bytes1) = to_ptr(&uref);

            let (item_name_ptr, item_name_size, _bytes2) = to_ptr(&name);

            let cl_value = CLValue::from_t(value).unwrap_or_revert();
            let (cl_value_ptr, cl_value_size, _bytes3) = to_ptr(&cl_value);

            for _i in 0..u64::MAX {
                let ret = unsafe {
                    ext_ffi::casper_dictionary_put(
                        uref_ptr,
                        uref_size,
                        item_name_ptr,
                        item_name_size,
                        cl_value_ptr,
                        cl_value_size,
                    )
                };
                api_error::result_from(ret).unwrap_or_revert();
            }
        }
        "load_call_stack" => {
            for _i in 0..u64::MAX {
                let call_stack = runtime::get_call_stack();
                assert_eq!(call_stack.len(), 1);
            }
        }
        "load_authorization_keys" => {
            let setup: bool = runtime::get_named_arg("setup");
            if setup {
                let weight = Weight::new(1);
                for i in 1..100 {
                    let account_hash = AccountHash::new([i; 32]);
                    account::add_associated_key(account_hash, weight).unwrap_or_revert();
                }
            } else {
                for _i in 0..u64::MAX {
                    let _k = runtime::list_authorization_keys();
                }
            }
        }
        "random_bytes" => {
            for _i in 0..u64::MAX {
                let _n = runtime::random_bytes();
            }
        }
        "dictionary_read" => {
            let name_len: u32 = runtime::get_named_arg("name_len");
            let name: String = "a".repeat(name_len as usize);
            let value_len: u32 = runtime::get_named_arg("value_len");
            let value = vec![u8::MAX; value_len as usize];
            let uref = storage::new_dictionary("a").unwrap_or_revert();
            storage::dictionary_put(uref, &name, value);
            let key = Key::dictionary(uref, name.as_bytes());

            for _i in 0..u64::MAX {
                let read_value: Vec<u8> = storage::dictionary_read(key)
                    .unwrap_or_revert()
                    .unwrap_or_revert();
                assert_eq!(read_value.len(), value_len as usize);
            }
        }
        "enable_contract_version" => {
            let (contract_pkg_hash, _uref) = storage::create_contract_package_at_hash();
            let (contract_hash, _version) = storage::add_contract_version(
                contract_pkg_hash,
                EntryPoints::new(),
                BTreeMap::new(),
            );
            for _i in 0..u64::MAX {
                storage::enable_contract_version(contract_pkg_hash, contract_hash)
                    .unwrap_or_revert();
            }
        }
        _ => panic!(),
    }
}

#[no_mangle]
extern "C" fn function_0() {}
#[no_mangle]
extern "C" fn function_1() {}
#[no_mangle]
extern "C" fn function_2() {}
#[no_mangle]
extern "C" fn function_3() {}
#[no_mangle]
extern "C" fn function_4() {}
#[no_mangle]
extern "C" fn function_5() {}
#[no_mangle]
extern "C" fn function_6() {}
#[no_mangle]
extern "C" fn function_7() {}
#[no_mangle]
extern "C" fn function_8() {}
#[no_mangle]
extern "C" fn function_9() {}
#[no_mangle]
extern "C" fn function_10() {}
#[no_mangle]
extern "C" fn function_11() {}
#[no_mangle]
extern "C" fn function_12() {}
#[no_mangle]
extern "C" fn function_13() {}
#[no_mangle]
extern "C" fn function_14() {}
#[no_mangle]
extern "C" fn function_15() {}
#[no_mangle]
extern "C" fn function_16() {}
#[no_mangle]
extern "C" fn function_17() {}
#[no_mangle]
extern "C" fn function_18() {}
#[no_mangle]
extern "C" fn function_19() {}
#[no_mangle]
extern "C" fn function_20() {}
#[no_mangle]
extern "C" fn function_21() {}
#[no_mangle]
extern "C" fn function_22() {}
#[no_mangle]
extern "C" fn function_23() {}
#[no_mangle]
extern "C" fn function_24() {}
#[no_mangle]
extern "C" fn function_25() {}
#[no_mangle]
extern "C" fn function_26() {}
#[no_mangle]
extern "C" fn function_27() {}
#[no_mangle]
extern "C" fn function_28() {}
#[no_mangle]
extern "C" fn function_29() {}
#[no_mangle]
extern "C" fn function_30() {}
#[no_mangle]
extern "C" fn function_31() {}
#[no_mangle]
extern "C" fn function_32() {}
#[no_mangle]
extern "C" fn function_33() {}
#[no_mangle]
extern "C" fn function_34() {}
#[no_mangle]
extern "C" fn function_35() {}
#[no_mangle]
extern "C" fn function_36() {}
#[no_mangle]
extern "C" fn function_37() {}
#[no_mangle]
extern "C" fn function_38() {}
#[no_mangle]
extern "C" fn function_39() {}
#[no_mangle]
extern "C" fn function_40() {}
#[no_mangle]
extern "C" fn function_41() {}
#[no_mangle]
extern "C" fn function_42() {}
#[no_mangle]
extern "C" fn function_43() {}
#[no_mangle]
extern "C" fn function_44() {}
#[no_mangle]
extern "C" fn function_45() {}
#[no_mangle]
extern "C" fn function_46() {}
#[no_mangle]
extern "C" fn function_47() {}
#[no_mangle]
extern "C" fn function_48() {}
#[no_mangle]
extern "C" fn function_49() {}
#[no_mangle]
extern "C" fn function_50() {}
#[no_mangle]
extern "C" fn function_51() {}
#[no_mangle]
extern "C" fn function_52() {}
#[no_mangle]
extern "C" fn function_53() {}
#[no_mangle]
extern "C" fn function_54() {}
#[no_mangle]
extern "C" fn function_55() {}
#[no_mangle]
extern "C" fn function_56() {}
#[no_mangle]
extern "C" fn function_57() {}
#[no_mangle]
extern "C" fn function_58() {}
#[no_mangle]
extern "C" fn function_59() {}
#[no_mangle]
extern "C" fn function_60() {}
#[no_mangle]
extern "C" fn function_61() {}
#[no_mangle]
extern "C" fn function_62() {}
#[no_mangle]
extern "C" fn function_63() {}
#[no_mangle]
extern "C" fn function_64() {}
#[no_mangle]
extern "C" fn function_65() {}
#[no_mangle]
extern "C" fn function_66() {}
#[no_mangle]
extern "C" fn function_67() {}
#[no_mangle]
extern "C" fn function_68() {}
#[no_mangle]
extern "C" fn function_69() {}
#[no_mangle]
extern "C" fn function_70() {}
#[no_mangle]
extern "C" fn function_71() {}
#[no_mangle]
extern "C" fn function_72() {}
#[no_mangle]
extern "C" fn function_73() {}
#[no_mangle]
extern "C" fn function_74() {}
#[no_mangle]
extern "C" fn function_75() {}
#[no_mangle]
extern "C" fn function_76() {}
#[no_mangle]
extern "C" fn function_77() {}
#[no_mangle]
extern "C" fn function_78() {}
#[no_mangle]
extern "C" fn function_79() {}
#[no_mangle]
extern "C" fn function_80() {}
#[no_mangle]
extern "C" fn function_81() {}
#[no_mangle]
extern "C" fn function_82() {}
#[no_mangle]
extern "C" fn function_83() {}
#[no_mangle]
extern "C" fn function_84() {}
#[no_mangle]
extern "C" fn function_85() {}
#[no_mangle]
extern "C" fn function_86() {}
#[no_mangle]
extern "C" fn function_87() {}
#[no_mangle]
extern "C" fn function_88() {}
#[no_mangle]
extern "C" fn function_89() {}
#[no_mangle]
extern "C" fn function_90() {}
#[no_mangle]
extern "C" fn function_91() {}
#[no_mangle]
extern "C" fn function_92() {}
#[no_mangle]
extern "C" fn function_93() {}
#[no_mangle]
extern "C" fn function_94() {}
#[no_mangle]
extern "C" fn function_95() {}
#[no_mangle]
extern "C" fn function_96() {}
#[no_mangle]
extern "C" fn function_97() {}
#[no_mangle]
extern "C" fn function_98() {}
#[no_mangle]
extern "C" fn function_99() {}
