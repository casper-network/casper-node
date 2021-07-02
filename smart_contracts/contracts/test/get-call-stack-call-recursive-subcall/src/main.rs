#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use casper_contract::contract_api::{runtime, storage};
use casper_types::{runtime_args, ApiError, Key, Phase, RuntimeArgs, U512};
use get_call_stack_recursive_subcall::{standard_payment, Call, ContractAddress};

const DEFAULT_PAYMENT: u64 = 1_500_000_000_000;
const ARG_CALLS: &str = "calls";
const ARG_CURRENT_DEPTH: &str = "current_depth";

#[no_mangle]
pub extern "C" fn call() {
    let calls: Vec<Call> = runtime::get_named_arg(ARG_CALLS);
    let current_depth: u8 = runtime::get_named_arg(ARG_CURRENT_DEPTH);

    // The important bit
    {
        let call_stack = runtime::get_call_stack();
        let name = alloc::format!("call_stack-{}", current_depth);
        let call_stack_at = storage::new_uref(call_stack);
        runtime::put_key(&name, Key::URef(call_stack_at));
    }

    if current_depth == 0 && runtime::get_phase() == Phase::Payment {
        standard_payment(U512::from(DEFAULT_PAYMENT))
    }

    if current_depth == calls.len() as u8 {
        return;
    }

    let args = runtime_args! {
        ARG_CALLS => calls.clone(),
        ARG_CURRENT_DEPTH => current_depth + 1,
    };

    match calls.get(current_depth as usize) {
        Some(Call {
            contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
            target_method,
            ..
        }) => {
            runtime::call_versioned_contract::<()>(
                *contract_package_hash,
                None,
                &target_method,
                args,
            );
        }
        Some(Call {
            contract_address: ContractAddress::ContractHash(contract_hash),
            target_method,
            ..
        }) => {
            runtime::call_contract::<()>(*contract_hash, &target_method, args);
        }
        _ => runtime::revert(ApiError::User(0)),
    }
}
