#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::String;

use casperlabs_contract::contract_api::{runtime, storage};

use casperlabs_types::{
    runtime_args, ApiError, CLType, ContractHash, ContractVersion, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Parameter, RuntimeArgs,
};

// This is making use of the undocumented "FFI" function `gas()` which is used by the Wasm
// interpreter to charge gas for upcoming interpreted instructions.  For further info on this, see
// https://docs.rs/pwasm-utils/0.12.0/pwasm_utils/fn.inject_gas_counter.html
mod unsafe_ffi {
    extern "C" {
        pub fn gas(amount: i32);
    }
}

fn safe_gas(amount: i32) {
    unsafe { unsafe_ffi::gas(amount) }
}

const SUBCALL_NAME: &str = "add_gas";
const ADD_GAS_FROM_SESSION: &str = "add-gas-from-session";
const ADD_GAS_VIA_SUBCALL: &str = "add-gas-via-subcall";

const ARG_GAS_AMOUNT: &str = "gas_amount";
const ARG_METHOD_NAME: &str = "method_name";

#[no_mangle]
pub extern "C" fn add_gas() {
    let amount: i32 = runtime::get_named_arg(ARG_GAS_AMOUNT);
    safe_gas(amount);
}

fn store() -> (ContractHash, ContractVersion) {
    let entry_points = {
        let mut entry_points = EntryPoints::new();
        let entry_point = EntryPoint::new(
            SUBCALL_NAME,
            vec![Parameter::new(ARG_GAS_AMOUNT, CLType::I32)],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(entry_point);

        entry_points
    };
    storage::new_contract(entry_points, None, None, None)
}

#[no_mangle]
pub extern "C" fn call() {
    let amount: i32 = runtime::get_named_arg(ARG_GAS_AMOUNT);
    let method_name: String = runtime::get_named_arg(ARG_METHOD_NAME);

    match method_name.as_str() {
        ADD_GAS_FROM_SESSION => safe_gas(amount),
        ADD_GAS_VIA_SUBCALL => {
            let (contract_hash, _contract_version) = store();
            runtime::call_contract(
                contract_hash,
                SUBCALL_NAME,
                runtime_args! { ARG_GAS_AMOUNT => amount, },
            )
        }
        _ => runtime::revert(ApiError::InvalidArgument),
    }
}
