#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use core::mem::MaybeUninit;

use alloc::string::String;
use casper_contract::{contract_api::runtime, ext_ffi, unwrap_or_revert::UnwrapOrRevert};
use casper_types::{
    api_error,
    bytesrepr::{self, Bytes, ToBytes},
    ApiError, ContractHash, RuntimeArgs,
};

const ARG_CONTRACT_HASH: &str = "contract_hash";
const ARG_ENTRYPOINT: &str = "entrypoint";
const ARG_ARGUMENTS: &str = "arguments";

// Generic call contract contract.
//
// Accepts entrypoint name, and saves possible return value into URef stored in named keys.
#[no_mangle]
pub extern "C" fn call() {
    let contract_hash: ContractHash = runtime::get_named_arg(ARG_CONTRACT_HASH);
    let entrypoint: String = runtime::get_named_arg(ARG_ENTRYPOINT);
    let arguments: RuntimeArgs = runtime::get_named_arg(ARG_ARGUMENTS);

    let _result_bytes = call_contract_forward(entrypoint, contract_hash, arguments);
}

fn deserialize_contract_result(bytes_written: usize) -> Option<Bytes> {
    if bytes_written == 0 {
        // If no bytes were written, the host buffer hasn't been set and hence shouldn't be read.
        None
    } else {
        // NOTE: this is a copy of the contents of `read_host_buffer()`.  Calling that directly from
        // here causes several contracts to fail with a Wasmi `Unreachable` error.
        let mut dest = vec![0; bytes_written];
        let real_size = read_host_buffer_into(&mut dest).unwrap_or_revert();
        assert_eq!(dest.len(), real_size);

        let bytes: Bytes = bytesrepr::deserialize_from_slice(&dest[..real_size]).unwrap_or_revert();

        Some(bytes)
    }
}

fn read_host_buffer_into(dest: &mut [u8]) -> Result<usize, ApiError> {
    let mut bytes_written = MaybeUninit::uninit();
    let ret = unsafe {
        ext_ffi::casper_read_host_buffer(dest.as_mut_ptr(), dest.len(), bytes_written.as_mut_ptr())
    };
    api_error::result_from(ret)?;
    Ok(unsafe { bytes_written.assume_init() })
}

/// Calls a contract and returns unwrapped [`CLValue`].
fn call_contract_forward(
    entrypoint: String,
    contract_hash: ContractHash,
    arguments: RuntimeArgs,
) -> Option<Bytes> {
    let entry_point_name: &str = &entrypoint;
    let contract_hash_ptr = contract_hash.to_bytes().unwrap_or_revert();
    let entry_point_name = entry_point_name.to_bytes().unwrap_or_revert();
    let runtime_args_ptr = arguments.to_bytes().unwrap_or_revert();
    let bytes_written = {
        let mut bytes_written = MaybeUninit::uninit();
        let ret = unsafe {
            ext_ffi::casper_call_contract(
                contract_hash_ptr.as_ptr(),
                contract_hash_ptr.len(),
                entry_point_name.as_ptr(),
                entry_point_name.len(),
                runtime_args_ptr.as_ptr(),
                runtime_args_ptr.len(),
                bytes_written.as_mut_ptr(),
            )
        };
        api_error::result_from(ret).unwrap_or_revert();
        unsafe { bytes_written.assume_init() }
    };
    deserialize_contract_result(bytes_written)
}
