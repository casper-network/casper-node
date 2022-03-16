#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};
use core::convert::TryInto;

use casper_contract::{
    contract_api::{self, runtime, storage},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    api_error, bytesrepr, contracts::NamedKeys, runtime_args, ApiError, CLType, CLTyped, CLValue,
    ContractPackageHash, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Key, Parameter,
    RuntimeArgs, URef,
};

const HASH_KEY_NAME: &str = "counter_package_hash";
const ACCESS_KEY_NAME: &str = "counter_package_access";
const CONTRACT_VERSION_KEY: &str = "contract_version";
const ENTRYPOINT_SESSION: &str = "session";
const ENTRYPOINT_COUNTER: &str = "counter";
const ARG_COUNTER_METHOD: &str = "method";
const ARG_CONTRACT_HASH_NAME: &str = "counter_contract_hash";
const COUNTER_VALUE_UREF: &str = "counter";
const METHOD_GET: &str = "get";
const METHOD_INC: &str = "inc";

#[no_mangle]
pub extern "C" fn counter() {
    let uref = runtime::get_key(COUNTER_VALUE_UREF)
        .unwrap_or_revert()
        .try_into()
        .unwrap_or_revert();

    let method_name: String = runtime::get_named_arg(ARG_COUNTER_METHOD);

    match method_name.as_str() {
        METHOD_INC => storage::add(uref, 1),
        METHOD_GET => {
            let result: i32 = storage::read_or_revert(uref);
            let return_value = CLValue::from_t(result).unwrap_or_revert();
            runtime::ret(return_value);
        }
        _ => runtime::revert(ApiError::InvalidArgument),
    }
}

#[no_mangle]
pub extern "C" fn session() {
    let counter_key = get_counter_key();
    let contract_hash = counter_key
        .into_hash()
        .unwrap_or_revert_with(ApiError::UnexpectedKeyVariant)
        .into();
    let entry_point_name = ENTRYPOINT_COUNTER;
    let runtime_args = runtime_args! { ARG_COUNTER_METHOD => METHOD_INC };
    runtime::call_contract(contract_hash, entry_point_name, runtime_args)
}

#[no_mangle]
pub extern "C" fn call() {
    let (contract_package_hash, access_uref): (ContractPackageHash, URef) =
        storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let entry_points = get_entry_points();
    let count_value_uref = storage::new_uref(0); //initialize counter
    let named_keys = {
        let mut ret = NamedKeys::new();
        ret.insert(String::from(COUNTER_VALUE_UREF), count_value_uref.into());
        ret
    };

    let (contract_hash, contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);
    let version_uref = storage::new_uref(contract_version);
    runtime::put_key(CONTRACT_VERSION_KEY, version_uref.into());
    runtime::put_key(ARG_CONTRACT_HASH_NAME, contract_hash.into());
}

fn get_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    // actual stored contract
    // ARG_METHOD -> METHOD_GET or METHOD_INC
    // ret -> counter value
    let entry_point = EntryPoint::new(
        ENTRYPOINT_COUNTER,
        vec![Parameter::new(ARG_COUNTER_METHOD, CLType::String)],
        CLType::I32,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    );
    entry_points.add_entry_point(entry_point);

    // stored session code that call a version of the stored contract
    // ARG_CONTRACT_HASH -> ContractHash of METHOD_COUNTER
    let entry_point = EntryPoint::new(
        ENTRYPOINT_SESSION,
        vec![Parameter::new(
            ARG_CONTRACT_HASH_NAME,
            Option::<Key>::cl_type(),
        )],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}

fn get_counter_key() -> Key {
    let name = ARG_CONTRACT_HASH_NAME;
    let arg = {
        let mut arg_size: usize = 0;
        let ret = unsafe {
            ext_ffi::casper_get_named_arg_size(
                name.as_bytes().as_ptr(),
                name.len(),
                &mut arg_size as *mut usize,
            )
        };
        match api_error::result_from(ret) {
            Ok(_) => {
                if arg_size == 0 {
                    None
                } else {
                    Some(arg_size)
                }
            }
            Err(ApiError::MissingArgument) => None,
            Err(e) => runtime::revert(e),
        }
    };

    match arg {
        Some(arg_size) => {
            let arg_bytes = {
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
                    let data = unsafe {
                        Vec::from_raw_parts(data_non_null_ptr.as_ptr(), arg_size, arg_size)
                    };
                    api_error::result_from(ret).map(|_| data)
                };
                res.unwrap_or_revert()
            };

            bytesrepr::deserialize(arg_bytes).unwrap_or_revert_with(ApiError::InvalidArgument)
        }
        None => runtime::get_key(ARG_CONTRACT_HASH_NAME).unwrap_or_revert_with(ApiError::GetKey),
    }
}
