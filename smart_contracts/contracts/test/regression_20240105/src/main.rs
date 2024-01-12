#![no_std]
#![no_main]

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::{iter, mem::MaybeUninit};

use casper_contract::{
    contract_api::{account, runtime, storage},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::{AccountHash, Weight},
    api_error,
    bytesrepr::ToBytes,
    runtime_args, AccessRights, ApiError, CLType, CLValue, ContractHash, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints, EraId, Key, RuntimeArgs, URef, U512,
};

const NOOP: &str = "noop";

fn to_ptr<T: ToBytes>(t: &T) -> (*const u8, usize, Vec<u8>) {
    let bytes = t.to_bytes().unwrap_or_revert();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size, bytes)
}

#[no_mangle]
pub extern "C" fn noop() {}

fn store_noop_contract() -> ContractHash {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(EntryPoint::new(
        NOOP,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    ));
    let (contract_hash, _version) = storage::new_contract(entry_points, None, None, None);
    contract_hash
}

fn get_name() -> String {
    let large_name: bool = runtime::get_named_arg("large_name");
    if large_name {
        iter::repeat('a').take(10_000).collect()
    } else {
        "a".to_string()
    }
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
            let contract_hash = store_noop_contract();
            let (contract_hash_ptr, contract_hash_size, _bytes1) = to_ptr(&contract_hash);
            let (entry_point_name_ptr, entry_point_name_size, _bytes2) = to_ptr(&NOOP);
            let (runtime_args_ptr, runtime_args_size, _bytes3) = to_ptr(&args);
            for _i in 0..u64::MAX {
                let mut bytes_written = MaybeUninit::uninit();
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
            for i in 0..u64::MAX {
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
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "set_action_threshold" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "load_named_keys" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_key" => {
            for _i in 0..u64::MAX {
                todo!()
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
                todo!()
            }
        }
        "transfer_to_account" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "transfer_from_purse_to_account" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "transfer_from_purse_to_purse" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_balance" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_phase" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_system_contract" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_main_purse" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "read_host_buffer" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "create_contract_package_at_hash" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "add_contract_version" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "disable_contract_version" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "call_versioned_contract" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "create_contract_user_group" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "print" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_runtime_arg_size" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "get_runtime_arg" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_contract_user_group" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "extend_contract_user_group_urefs" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "remove_contract_user_group_urefs" => {
            for _i in 0..u64::MAX {
                todo!()
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
            for i in 0..u64::MAX {
                let _uref = storage::new_dictionary(&i.to_string()).unwrap(); // 1:40
            }
        }
        "dictionary_get" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "dictionary_put" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "load_call_stack" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "load_authorization_keys" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "random_bytes" => {
            for _i in 0..u64::MAX {
                let _n = runtime::random_bytes(); // 0:05
            }
        }
        "dictionary_read" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        "enable_contract_version" => {
            for _i in 0..u64::MAX {
                todo!()
            }
        }
        _ => panic!(),
    }
}
