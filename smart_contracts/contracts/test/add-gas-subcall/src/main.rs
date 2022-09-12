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

/// This should consume at least `amount * gas_per_byte + C` gas
/// where C contains wasm overhead and host function calls.
fn consume_at_least_gas_amount(amount: usize) {
    if amount > 0 {
        let data_uref = match runtime::get_key(DATA_KEY) {
            Some(Key::URef(uref)) => uref,
            Some(_key) => runtime::revert(ApiError::UnexpectedKeyVariant),
            None => {
                let uref = storage::new_uref(());
                runtime::put_key(DATA_KEY, uref.into());
                uref
            }
        };

        let data = vec![0; amount];
        storage::write(data_uref, data);
    }
}

#[no_mangle]
pub extern "C" fn add_gas() {
    let amount: u32 = runtime::get_named_arg(ARG_GAS_AMOUNT);

    consume_at_least_gas_amount(amount as usize);
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
        ADD_GAS_FROM_SESSION => consume_at_least_gas_amount(amount as usize),
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
