#![no_std]
#![no_main]

#[macro_use]
extern crate alloc;

use alloc::string::String;

use casper_contract::contract_api::{runtime, storage};

use casper_types::{
    runtime_args, ApiError, CLType, ContractHash, ContractVersion, EntryPoint, EntryPointAccess,
    EntryPointType, EntryPoints, Key, Parameter, RuntimeArgs,
};

const SUBCALL_NAME: &str = "add_gas";
const DATA_KEY: &str = "data";
const ADD_GAS_FROM_SESSION: &str = "add-gas-from-session";
const ADD_GAS_VIA_SUBCALL: &str = "add-gas-via-subcall";

const ARG_GAS_AMOUNT: &str = "gas_amount";
const ARG_METHOD_NAME: &str = "method_name";

fn consume_gas(amount: i32) {
    if amount > 0 {
        let data = vec![0u8; amount as usize];
        let data_uref = runtime::get_key(DATA_KEY)
            .and_then(Key::into_uref)
            .unwrap_or_else(|| storage::new_uref(()));

        storage::write(data_uref, data);
    }
}

#[no_mangle]
pub extern "C" fn add_gas() {
    let amount: i32 = runtime::get_named_arg(ARG_GAS_AMOUNT);

    consume_gas(amount);
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
        ADD_GAS_FROM_SESSION => consume_gas(amount),
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
