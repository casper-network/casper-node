#![no_std]
#![no_main]
#![allow(unused_imports)]

extern crate alloc;

// Can be removed once https://github.com/rust-lang/rustfmt/issues/3362 is resolved.
#[rustfmt::skip]
use alloc::vec;
use alloc::{collections::BTreeMap, vec::Vec};

use casperlabs_contract::{
    self,
    contract_api::{self, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    api_error,
    bytesrepr::{FromBytes, ToBytes},
    contracts::Parameters,
    CLType, CLTyped, ContractHash, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key,
    RuntimeArgs,
};
use contract_api::runtime;

#[no_mangle]
pub extern "C" fn do_nothing() {
    // A function that does nothing.
    // This is used to just pass the checks in `call_contract` on the host side.
}

// Attacker copied to_ptr from `alloc_utils` as it was private
fn to_ptr<T: ToBytes>(t: T) -> (*const u8, usize, Vec<u8>) {
    let bytes = t.into_bytes().unwrap_or_revert();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size, bytes)
}

mod malicious_ffi {
    // Potential attacker has available every FFI for himself
    extern "C" {
        pub fn call_contract(
            contract_hash_ptr: *const u8,
            contract_hash_size: usize,
            entry_point_name_ptr: *const u8,
            entry_point_name_size: usize,
            runtime_args_ptr: *const u8,
            runtime_args_size: usize,
            result_size: *mut usize,
        ) -> i32;
    }
}

// This is half-baked runtime::call_contract with changed `extra_urefs`
// parameter with a desired payload that's supposed to bring the node down.
pub fn my_call_contract(
    contract_hash: ContractHash,
    _entry_point_name: &str,
    runtime_args: RuntimeArgs,
) -> usize {
    let (contract_hash_ptr, contract_hash_size, _bytes1) = to_ptr(contract_hash);

    let malicious_string = vec![255, 255, 255, 255, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

    let (runtime_args_ptr, runtime_args_size, _bytes2) = to_ptr(runtime_args);

    {
        let mut bytes_written = 0usize;
        let ret = unsafe {
            malicious_ffi::call_contract(
                contract_hash_ptr,
                contract_hash_size,
                malicious_string.as_ptr(),
                malicious_string.len(),
                runtime_args_ptr,
                runtime_args_size,
                &mut bytes_written as *mut usize,
            )
        };
        api_error::result_from(ret).unwrap_or_revert();
        bytes_written
    }
}

#[no_mangle]
pub extern "C" fn call() {
    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let entry_point = EntryPoint::new(
            "do_nothing",
            Parameters::default(),
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    let (contract_hash, _contract_version) = storage::new_contract(entry_points, None, None, None);

    my_call_contract(contract_hash, "do_nothing", RuntimeArgs::default());
}
