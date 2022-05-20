//! Functions for interacting with the current runtime.

use alloc::{collections::BTreeSet, vec, vec::Vec};
use core::mem::MaybeUninit;

use casper_types::{
    account::AccountHash,
    api_error,
    bytesrepr::{self, FromBytes},
    contracts::{ContractVersion, NamedKeys},
    system::CallStackElement,
    ApiError, BlockTime, CLTyped, CLValue, ContractHash, ContractPackageHash, Key, Phase,
    RuntimeArgs, URef, BLAKE2B_DIGEST_LENGTH, BLOCKTIME_SERIALIZED_LENGTH, PHASE_SERIALIZED_LENGTH,
};

use crate::{contract_api, ext_ffi, unwrap_or_revert::UnwrapOrRevert};

/// Number of random bytes returned from the `random_bytes()` function.
const RANDOM_BYTES_COUNT: usize = 32;

/// Returns the given [`CLValue`] to the host, terminating the currently running module.
///
/// Note this function is only relevant to contracts stored on chain which are invoked via
/// [`call_contract`] and can thus return a value to their caller.  The return value of a directly
/// deployed contract is never used.
pub fn ret(value: CLValue) -> ! {
    let (ptr, size, _bytes) = contract_api::to_ptr(value);
    unsafe {
        ext_ffi::casper_ret(ptr, size);
    }
}

/// Stops execution of a contract and reverts execution effects with a given [`ApiError`].
///
/// The provided `ApiError` is returned in the form of a numeric exit code to the caller via the
/// deploy response.
pub fn revert<T: Into<ApiError>>(error: T) -> ! {
    unsafe {
        ext_ffi::casper_revert(error.into().into());
    }
}

/// Calls the given stored contract, passing the given arguments to it.
///
/// If the stored contract calls [`ret`], then that value is returned from `call_contract`.  If the
/// stored contract calls [`revert`], then execution stops and `call_contract` doesn't return.
/// Otherwise `call_contract` returns `()`.
pub fn call_contract<T: CLTyped + FromBytes>(
    contract_hash: ContractHash,
    entry_point_name: &str,
    runtime_args: RuntimeArgs,
) -> T {
    let (contract_hash_ptr, contract_hash_size, _bytes1) = contract_api::to_ptr(contract_hash);
    let (entry_point_name_ptr, entry_point_name_size, _bytes2) =
        contract_api::to_ptr(entry_point_name);
    let (runtime_args_ptr, runtime_args_size, _bytes2) = contract_api::to_ptr(runtime_args);

    let bytes_written = {
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
        unsafe { bytes_written.assume_init() }
    };
    deserialize_contract_result(bytes_written)
}

/// Invokes the specified `entry_point_name` of stored logic at a specific `contract_package_hash`
/// address, for the most current version of a contract package by default or a specific
/// `contract_version` if one is provided, and passing the provided `runtime_args` to it
///
/// If the stored contract calls [`ret`], then that value is returned from
/// `call_versioned_contract`.  If the stored contract calls [`revert`], then execution stops and
/// `call_versioned_contract` doesn't return. Otherwise `call_versioned_contract` returns `()`.
pub fn call_versioned_contract<T: CLTyped + FromBytes>(
    contract_package_hash: ContractPackageHash,
    contract_version: Option<ContractVersion>,
    entry_point_name: &str,
    runtime_args: RuntimeArgs,
) -> T {
    let (contract_package_hash_ptr, contract_package_hash_size, _bytes) =
        contract_api::to_ptr(contract_package_hash);
    let (contract_version_ptr, contract_version_size, _bytes) =
        contract_api::to_ptr(contract_version);
    let (entry_point_name_ptr, entry_point_name_size, _bytes) =
        contract_api::to_ptr(entry_point_name);
    let (runtime_args_ptr, runtime_args_size, _bytes) = contract_api::to_ptr(runtime_args);

    let bytes_written = {
        let mut bytes_written = MaybeUninit::uninit();
        let ret = unsafe {
            ext_ffi::casper_call_versioned_contract(
                contract_package_hash_ptr,
                contract_package_hash_size,
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
        unsafe { bytes_written.assume_init() }
    };
    deserialize_contract_result(bytes_written)
}

fn deserialize_contract_result<T: CLTyped + FromBytes>(bytes_written: usize) -> T {
    let serialized_result = if bytes_written == 0 {
        // If no bytes were written, the host buffer hasn't been set and hence shouldn't be read.
        vec![]
    } else {
        // NOTE: this is a copy of the contents of `read_host_buffer()`.  Calling that directly from
        // here causes several contracts to fail with a Wasmi `Unreachable` error.
        let bytes_non_null_ptr = contract_api::alloc_bytes(bytes_written);
        let mut dest: Vec<u8> = unsafe {
            Vec::from_raw_parts(bytes_non_null_ptr.as_ptr(), bytes_written, bytes_written)
        };
        read_host_buffer_into(&mut dest).unwrap_or_revert();
        dest
    };

    bytesrepr::deserialize(serialized_result).unwrap_or_revert()
}

fn get_named_arg_size(name: &str) -> Option<usize> {
    let mut arg_size: usize = 0;
    let ret = unsafe {
        ext_ffi::casper_get_named_arg_size(
            name.as_bytes().as_ptr(),
            name.len(),
            &mut arg_size as *mut usize,
        )
    };
    match api_error::result_from(ret) {
        Ok(_) => Some(arg_size),
        Err(ApiError::MissingArgument) => None,
        Err(e) => revert(e),
    }
}

/// Returns given named argument passed to the host for the current module invocation.
///
/// Note that this is only relevant to contracts stored on-chain since a contract deployed directly
/// is not invoked with any arguments.
pub fn get_named_arg<T: FromBytes>(name: &str) -> T {
    let arg_size = get_named_arg_size(name).unwrap_or_revert_with(ApiError::MissingArgument);
    let arg_bytes = if arg_size > 0 {
        let res = {
            let data_non_null_ptr = contract_api::alloc_bytes(arg_size);
            let ret = unsafe {
                ext_ffi::casper_get_named_arg(
                    name.as_bytes().as_ptr(),
                    name.len(),
                    data_non_null_ptr.as_ptr(),
                    arg_size,
                )
            };
            let data =
                unsafe { Vec::from_raw_parts(data_non_null_ptr.as_ptr(), arg_size, arg_size) };
            api_error::result_from(ret).map(|_| data)
        };
        // Assumed to be safe as `get_named_arg_size` checks the argument already
        res.unwrap_or_revert()
    } else {
        // Avoids allocation with 0 bytes and a call to get_named_arg
        Vec::new()
    };
    bytesrepr::deserialize(arg_bytes).unwrap_or_revert_with(ApiError::InvalidArgument)
}

/// Returns the caller of the current context, i.e. the [`AccountHash`] of the account which made
/// the deploy request.
pub fn get_caller() -> AccountHash {
    let output_size = {
        let mut output_size = MaybeUninit::uninit();
        let ret = unsafe { ext_ffi::casper_get_caller(output_size.as_mut_ptr()) };
        api_error::result_from(ret).unwrap_or_revert();
        unsafe { output_size.assume_init() }
    };
    let buf = read_host_buffer(output_size).unwrap_or_revert();
    bytesrepr::deserialize(buf).unwrap_or_revert()
}

/// Returns the current [`BlockTime`].
pub fn get_blocktime() -> BlockTime {
    let dest_non_null_ptr = contract_api::alloc_bytes(BLOCKTIME_SERIALIZED_LENGTH);
    let bytes = unsafe {
        ext_ffi::casper_get_blocktime(dest_non_null_ptr.as_ptr());
        Vec::from_raw_parts(
            dest_non_null_ptr.as_ptr(),
            BLOCKTIME_SERIALIZED_LENGTH,
            BLOCKTIME_SERIALIZED_LENGTH,
        )
    };
    bytesrepr::deserialize(bytes).unwrap_or_revert()
}

/// Returns the current [`Phase`].
pub fn get_phase() -> Phase {
    let dest_non_null_ptr = contract_api::alloc_bytes(PHASE_SERIALIZED_LENGTH);
    unsafe { ext_ffi::casper_get_phase(dest_non_null_ptr.as_ptr()) };
    let bytes = unsafe {
        Vec::from_raw_parts(
            dest_non_null_ptr.as_ptr(),
            PHASE_SERIALIZED_LENGTH,
            PHASE_SERIALIZED_LENGTH,
        )
    };
    bytesrepr::deserialize(bytes).unwrap_or_revert()
}

/// Returns the requested named [`Key`] from the current context.
///
/// The current context is either the caller's account or a stored contract depending on whether the
/// currently-executing module is a direct call or a sub-call respectively.
pub fn get_key(name: &str) -> Option<Key> {
    let (name_ptr, name_size, _bytes) = contract_api::to_ptr(name);
    let mut key_bytes = vec![0u8; Key::max_serialized_length()];
    let mut total_bytes: usize = 0;
    let ret = unsafe {
        ext_ffi::casper_get_key(
            name_ptr,
            name_size,
            key_bytes.as_mut_ptr(),
            key_bytes.len(),
            &mut total_bytes as *mut usize,
        )
    };
    match api_error::result_from(ret) {
        Ok(_) => {}
        Err(ApiError::MissingKey) => return None,
        Err(e) => revert(e),
    }
    key_bytes.truncate(total_bytes);
    let key: Key = bytesrepr::deserialize(key_bytes).unwrap_or_revert();
    Some(key)
}

/// Returns `true` if `name` exists in the current context's named keys.
///
/// The current context is either the caller's account or a stored contract depending on whether the
/// currently-executing module is a direct call or a sub-call respectively.
pub fn has_key(name: &str) -> bool {
    let (name_ptr, name_size, _bytes) = contract_api::to_ptr(name);
    let result = unsafe { ext_ffi::casper_has_key(name_ptr, name_size) };
    result == 0
}

/// Stores the given [`Key`] under `name` in the current context's named keys.
///
/// The current context is either the caller's account or a stored contract depending on whether the
/// currently-executing module is a direct call or a sub-call respectively.
pub fn put_key(name: &str, key: Key) {
    let (name_ptr, name_size, _bytes) = contract_api::to_ptr(name);
    let (key_ptr, key_size, _bytes2) = contract_api::to_ptr(key);
    unsafe { ext_ffi::casper_put_key(name_ptr, name_size, key_ptr, key_size) };
}

/// Removes the [`Key`] stored under `name` in the current context's named keys.
///
/// The current context is either the caller's account or a stored contract depending on whether the
/// currently-executing module is a direct call or a sub-call respectively.
pub fn remove_key(name: &str) {
    let (name_ptr, name_size, _bytes) = contract_api::to_ptr(name);
    unsafe { ext_ffi::casper_remove_key(name_ptr, name_size) }
}

/// Returns the set of [`AccountHash`] from the calling account's context `authorization_keys`.
pub fn list_authorization_keys() -> BTreeSet<AccountHash> {
    let (total_authorization_keys, result_size) = {
        let mut authorization_keys = MaybeUninit::uninit();
        let mut result_size = MaybeUninit::uninit();
        let ret = unsafe {
            ext_ffi::casper_load_authorization_keys(
                authorization_keys.as_mut_ptr(),
                result_size.as_mut_ptr(),
            )
        };
        api_error::result_from(ret).unwrap_or_revert();
        let total_authorization_keys = unsafe { authorization_keys.assume_init() };
        let result_size = unsafe { result_size.assume_init() };
        (total_authorization_keys, result_size)
    };

    if total_authorization_keys == 0 {
        return BTreeSet::new();
    }

    let bytes = read_host_buffer(result_size).unwrap_or_revert();
    bytesrepr::deserialize(bytes).unwrap_or_revert()
}

/// Returns the named keys of the current context.
///
/// The current context is either the caller's account or a stored contract depending on whether the
/// currently-executing module is a direct call or a sub-call respectively.
pub fn list_named_keys() -> NamedKeys {
    let (total_keys, result_size) = {
        let mut total_keys = MaybeUninit::uninit();
        let mut result_size = 0;
        let ret = unsafe {
            ext_ffi::casper_load_named_keys(total_keys.as_mut_ptr(), &mut result_size as *mut usize)
        };
        api_error::result_from(ret).unwrap_or_revert();
        let total_keys = unsafe { total_keys.assume_init() };
        (total_keys, result_size)
    };
    if total_keys == 0 {
        return NamedKeys::new();
    }
    let bytes = read_host_buffer(result_size).unwrap_or_revert();
    bytesrepr::deserialize(bytes).unwrap_or_revert()
}

/// Validates uref against named keys.
pub fn is_valid_uref(uref: URef) -> bool {
    let (uref_ptr, uref_size, _bytes) = contract_api::to_ptr(uref);
    let result = unsafe { ext_ffi::casper_is_valid_uref(uref_ptr, uref_size) };
    result != 0
}

/// Returns a 32-byte BLAKE2b digest
pub fn blake2b<T: AsRef<[u8]>>(input: T) -> [u8; BLAKE2B_DIGEST_LENGTH] {
    let mut ret = [0; BLAKE2B_DIGEST_LENGTH];
    let result = unsafe {
        ext_ffi::casper_blake2b(
            input.as_ref().as_ptr(),
            input.as_ref().len(),
            ret.as_mut_ptr(),
            BLAKE2B_DIGEST_LENGTH,
        )
    };
    api_error::result_from(result).unwrap_or_revert();
    ret
}

/// Returns 32 pseudo random bytes.
pub fn random_bytes() -> [u8; RANDOM_BYTES_COUNT] {
    let mut ret = [0; RANDOM_BYTES_COUNT];
    let result = unsafe { ext_ffi::casper_random_bytes(ret.as_mut_ptr(), RANDOM_BYTES_COUNT) };
    api_error::result_from(result).unwrap_or_revert();
    ret
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

pub(crate) fn read_host_buffer(size: usize) -> Result<Vec<u8>, ApiError> {
    let mut dest: Vec<u8> = if size == 0 {
        Vec::new()
    } else {
        let bytes_non_null_ptr = contract_api::alloc_bytes(size);
        unsafe { Vec::from_raw_parts(bytes_non_null_ptr.as_ptr(), size, size) }
    };
    read_host_buffer_into(&mut dest)?;
    Ok(dest)
}

/// Returns the call stack.
pub fn get_call_stack() -> Vec<CallStackElement> {
    let (call_stack_len, result_size) = {
        let mut call_stack_len: usize = 0;
        let mut result_size: usize = 0;
        let ret = unsafe {
            ext_ffi::casper_load_call_stack(
                &mut call_stack_len as *mut usize,
                &mut result_size as *mut usize,
            )
        };
        api_error::result_from(ret).unwrap_or_revert();
        (call_stack_len, result_size)
    };
    if call_stack_len == 0 {
        return Vec::new();
    }
    let bytes = read_host_buffer(result_size).unwrap_or_revert();
    bytesrepr::deserialize(bytes).unwrap_or_revert()
}

#[cfg(feature = "test-support")]
/// Prints a debug message
pub fn print(text: &str) {
    let (text_ptr, text_size, _bytes) = contract_api::to_ptr(text);
    unsafe { ext_ffi::casper_print(text_ptr, text_size) }
}
